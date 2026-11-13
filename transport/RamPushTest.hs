{-# LANGUAGE OverloadedStrings #-}
module Main where

import Control.Exception (SomeException, throwIO, try)
import Control.Monad (forM_)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import Data.IORef
import Data.List (isInfixOf)
import Data.Aeson (encode, object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.ByteString.Lazy as BL
import Data.Word (Word32)
import Numeric (showHex)
import System.Directory
import System.FilePath
import System.Posix.Files (fileMode, getFileStatus, intersectFileModes, setFileMode)
import FujiAF.TestScratch (scratchDirectory)
import FujiAF.RamPush
import FujiAF.SHA256 (sha256)
import FujiAF.Rehearsal (rehearseRamAlloc, rehearseRamPush, rehearseRamFree, rehearseRamUnload)
import FujiAF.Transport
import qualified FujiAF.Broker as P
import RamPushLayout (installerInstallOffset, installerRollbackOffset, installerStatusOffset, runtimeAllocationSize, runtimeAllocationSizeFor, runtimeInstallerBase, standardBankVariant, modalScopedBankVariant, lifecycleBankVariant, modalLifecycleBankVariant, modalEngineBankVariant, modalRenderBankVariant, modalValueBankVariant, bankVariantTag)
import qualified RamPushLayout as Layout
import qualified Replacement.GeneralHeap.Admission as Heap
import RamPushModel

contract :: ReviewedContract
contract = ReviewedContract
  [ ("build-recipe-bank",replicate 64 'c'), ("menu/ram_push_installer.c",replicate 64 'd'), ("ram-push-toolchain",replicate 64 'e') ]
  (replicate 64 '0')

makeDir :: FilePath -> IO ()
makeDir path = createDirectory path >> setFileMode path 0o700

runAlloc :: FilePath -> Model -> IO RamAllocOutcome
runAlloc directory model = runRamAllocWith PeerCacheBound reviewedAllocationSize (Clock (pure 0)) (modelApi model) directory

runPush :: FilePath -> Model -> RamAllocState -> IO RamPushOutcome
runPush directory model state = runRamPushWith PeerCacheBound NoRamPushObserver state manifest (Clock (pure 0)) (modelApi model) directory

data RuntimeUploadState = RuntimeIncomplete | RuntimeUploaded deriving (Eq, Show)

data RamQueryObservation = RamQueryObservation Address RuntimeUploadState deriving (Eq, Show)

observeRamQueries :: Model -> IO (UsbApi, IO [RamQueryObservation])
observeRamQueries model = do
  pending <- newIORef Nothing
  completed <- newIORef []
  let api = modelApi model
      observed = api { usbTransfer = \transfer timeout -> do
        query <- case transfer of
          P.BulkOut _ frame | length frame >= 32 && u32 0 frame == 0x450006 -> do
            target <- Address . u32 0 <$> readBytes (modelMemory model) (unAddress callbackSlotAddress) 4
            bytes <- B.pack <$> readBytes (modelMemory model) base (B.length runtime)
            pure (Just (RamQueryObservation target (if bytes == runtime then RuntimeUploaded else RuntimeIncomplete)))
          _ -> pure Nothing
        result <- usbTransfer api transfer timeout
        case transfer of
          P.BulkOut _ _ -> writeIORef pending query
          P.BulkIn _ _ -> do
            reply <- readIORef pending
            forM_ reply $ \observation -> modifyIORef' completed (++ [observation])
            writeIORef pending Nothing
          _ -> pure ()
        pure result }
  pure (observed, readIORef completed)

uploadedSynchronization :: [RamQueryObservation]
uploadedSynchronization =
  [ RamQueryObservation (Address 0x0061c4f4) RuntimeUploaded
  , RamQueryObservation (Address 0x0061c490) RuntimeUploaded
  ]

checkUnpublished :: String -> Model -> IO ()
checkUnpublished label model = do
  sent <- readIORef (modelHostWrites model)
  forM_ publications $ \publication -> do
    let address = unAddress (publicationAddress publication)
        outside (start, bytes) = start + fromIntegral (length bytes) <= address || start >= address + 4
    actual <- u32 0 <$> readBytes (modelMemory model) address 4
    check (label ++ ": publication word remains old") (actual == unWordValue (publicationOld publication))
    check (label ++ ": no host write touches a publication word") (all outside sent)
  let installerRetarget = (unAddress callbackSlotAddress, word32le (unAddress (installEntry (manifestEntries manifest))))
  check (label ++ ": installer callback has not been sent") (installerRetarget `notElem` sent)

manifestDocument :: RamPushManifest -> B.ByteString
manifestDocument candidate = BL.toStrict (encode (object $
  [ "schema" .= (1 :: Int)
  , "base" .= unAddressHex (allocationBase candidate)
  , "size" .= allocationSize candidate
  , "runtime_sha256" .= runtimeDigest candidate
  , "source_identity" .= object [K.fromString name .= digest | (name,digest) <- sourceIdentity candidate]
  , "entry" .= object
      [ "install" .= unAddressHex (installEntry entries)
      , "rollback" .= unAddressHex (rollbackEntry entries)
      , "status" .= unAddressHex (statusEntry entries) ]
  , "publications" .= map publication (manifestPublications candidate) ]
  ++ ["bank_variant" .= bankVariantTag variant | Just variant <- [manifestBankVariant candidate], variant `elem` [modalRenderBankVariant, modalValueBankVariant]]))
  where
    entries = manifestEntries candidate
    unAddressHex = hexWord . unAddress
    hexWord value = "0x" ++ replicate (8 - length digits) '0' ++ digits where digits = showHex value ""
    publication p = object
      [ "index" .= publicationIndex p
      , "phase" .= (case publicationPhase p of Setup -> "setup" :: String; Activation -> "activation")
      , "address" .= unAddressHex (publicationAddress p)
      , "old" .= hexWord (unWordValue (publicationOld p))
      , "new" .= hexWord (unWordValue (publicationNew p))
      , "core" .= (case publicationCore p of Cpu0 -> "cpu0" :: String; Cpu1 -> "cpu1"; Both -> "both"; UnknownCore -> "unknown") ]

main :: IO ()
main = do
  root <- scratchDirectory "fuji-af-ram-push-tests-XXXXXX"
  setFileMode root 0o700

  propertyLayout <- either (error . ("property layout: " ++)) pure (Layout.propertyPublicationLayout modalRenderBankVariant)
  (_, propertyArena) <- either (error . show) pure (Heap.decodeGeometry
    [0x48454150,1,0xa71a00,0xbf1a00,0x470,0xa71e70,32,17,16,16,16,8,32,16])
  check "property-only extent is not a normal payload or camera CLI allocation"
    (Layout.propertyAllocationSize propertyLayout base == Right 253056
      && fmap payloadKind (payloadOfSize 253056) == Nothing && ramAllocArguments ["253056"] == Nothing)
  let propertyDir = root </> "property-only"
      propertyClock = Clock (pure 0)
  makeDir propertyDir
  propertyModel <- newModel Nothing
  forM_ (zip [base-16,base-12..base-4] [0x584d3148,0x3dc80,0x34a2c8a6,0x3dc89]) $ \(address, value) ->
    writeWord (modelMemory propertyModel) address value
  propertyOutcome <- runPropertyAllocationWith PeerCacheBound propertyLayout propertyArena propertyClock (modelApi propertyModel) propertyDir
  propertyPath <- case propertyOutcome of
    RamAllocated path -> pure path
    other -> error ("property allocation failed: " ++ show other)
  propertyState <- readRamAllocState propertyPath
  check "property-only allocation returns its actual extent, base and whole-block guard"
    (allocSize propertyState == 253056 && allocBase propertyState == Address base
      && allocGuard propertyState == sha256 (B.replicate 253056 0))
  propertyDocument <- B.readFile propertyPath
  check "property state is allocation-only" ("xm1-property-allocation-only-state" `B.isInfixOf` propertyDocument)
  propertyPush <- runRamPushWith PeerCacheBound NoRamPushObserver propertyState manifest propertyClock (modelApi propertyModel) propertyDir
  check "property allocation is not installable through normal RAM push"
    (case propertyPush of RamPushRefused _ -> True; _ -> False)
  forM_
    [ ("size", propertyState { allocSize = 253048 })
    , ("base", propertyState { allocBase = Address 0xfffffff8 })
    , ("identity", propertyState { allocProduct = "X-T1" })
    , ("core", propertyState { allocCpu0 = 0x4004 })
    , ("bootstrap", propertyState { allocBootstrap = B.replicate 32 0 })
    , ("slot", propertyState { allocSlotWord = WordValue 0 })
    ] $ \(label, invalid) -> do
      let directory = root </> ("property-state-" ++ label)
      makeDir directory
      path <- writeAllocStateFile directory invalid
      parsed <- try (readRamAllocState path) :: IO (Either SomeException RamAllocState)
      check ("property allocation state rejects " ++ label) (case parsed of Left _ -> True; Right _ -> False)
  forM_
    [ ("peer", PeerCacheUnbound, \_ -> pure ())
    , ("identity", PeerCacheBound, \model -> modifyIORef' (modelIdentity model) (\bytes -> take 2 bytes ++ [0] ++ drop 3 bytes))
    , ("f7", PeerCacheBound, \model -> modifyIORef' (modelConfig model) (\bytes -> take (36+0xf7) bytes ++ [0] ++ drop (37+0xf7) bytes))
    , ("session", PeerCacheBound, \model -> writeWord (modelMemory model) (unAddress callbackTablePointerAddress) 0)
    ] $ \(label, peer, change) -> do
      let directory = root </> ("property-refusal-" ++ label)
      makeDir directory
      model <- newModel Nothing
      change model
      result <- runPropertyAllocationWith peer propertyLayout propertyArena propertyClock (modelApi model) directory
      check ("property allocation refuses " ++ label) (case result of RamAllocRefused _ -> True; _ -> False)
      readIORef (modelHostWrites model) >>= check ("property refusal writes nothing: " ++ label) . null
  let invalidHeaderDir = root </> "property-invalid-header"
  makeDir invalidHeaderDir
  invalidHeaderModel <- newModel Nothing
  invalidHeaderOutcome <- runPropertyAllocationWith PeerCacheBound propertyLayout propertyArena propertyClock (modelApi invalidHeaderModel) invalidHeaderDir
  check "returned pointer without owned heap header is not admitted"
    (case invalidHeaderOutcome of RamAllocUncertain _ -> True; _ -> False)
  check "bootstraps are exactly 32 bytes" (all ((==32) . B.length) [bootstrapBytes,peerPrepareBytes,freeBootstrapBytes])
  check "allocator preserves callback output pointer" (B.take 4 (B.drop 4 bootstrapBytes) == B.pack [0x01,0x50,0xa0,0xe1])
  check "peer preparation calls the worker unconditionally" (B.index peerPrepareBytes 19 == 0xeb)
  check "CLI accepts split flow" (ramAllocArguments ["0x1dc98"] == Just (reviewedAllocationSize,Nothing)
    && ramAllocArguments ["0x1d9d8"] == Nothing
    && ramPushArguments ["/a","/m"] == Just ("/a","/m",Nothing)
    && ramPushArguments ["/a","/m","/o"] == Just ("/a","/m",Just "/o")
    && ramFreeArguments ["/a"] == Just ("/a",Nothing))
  check "CLI accepts the USB toggle allocation extent"
    (ramAllocArguments ["0xd20"] == Just (0xd20, Nothing)
      && ramAllocArguments ["3360", "/toggle-allocation"] == Just (0xd20, Just "/toggle-allocation")
      && ramAllocArguments ["0xd00"] == Nothing
      && ramAllocArguments ["0xd28"] == Nothing
      && fmap payloadKind (payloadOfSize 0xd20) == Just UsbToggle)
  check "CLI rejects flags, non-reviewed sizes and excess arguments" (ramAllocArguments ["0"] == Nothing
    && ramAllocArguments ["0x1a99f"] == Nothing && ramAllocArguments ["0x1a9a1"] == Nothing
    && ramPushArguments ["/m"] == Nothing && ramPushArguments ["--raw","/m"] == Nothing
    && ramPushArguments ["/a","/m","/o","x"] == Nothing)
  let alignedBases = [0x00a00000 + low | low <- [0, 8 .. 0xfff8]] :: [Word32]
      layoutEntries kind at =
        let start = payloadInstallerStart kind at
        in case payloadKind kind of
          AfRing -> RamPushEntries (Address (start + 0x600)) (Address (start + 0x834)) (Address (start + 0x860))
          UsbToggle -> RamPushEntries (Address (start + 0x600)) (Address (start + 0x83c)) (Address (start + 0x868))
          _ -> RamPushEntries (Address (start + installerInstallOffset)) (Address (start + installerRollbackOffset)) (Address (start + installerStatusOffset))
  check "every payload's entries match its compiled installer layout"
    (and [payloadEntries kind (Address at) == layoutEntries kind at | kind <- payloads, at <- alignedBases])
  check "the bank installer uses RamPushLayout's separate installer base"
    (all (\at -> payloadInstallerStart bankD3Payload at == runtimeInstallerBase at) alignedBases)
  check "the bank allocation is RamPushLayout's"
    (payloadAllocationSize bankD3Payload == fromIntegral runtimeAllocationSize)
  check "USB toggle exposes 22 setup publications then one activation"
    (payloadSetupCount usbTogglePayload == 22 && payloadActivationCount usbTogglePayload == 1
      && payloadPublicationCount usbTogglePayload == 23 && payloadAbiVersion usbTogglePayload == 6)
  check "USB toggle's installer fits every accepted base alignment"
    (all (\at -> toInteger (payloadInstallerStart usbTogglePayload at) + 0xd00
        <= toInteger at + toInteger (payloadAllocationSize usbTogglePayload)) alignedBases)

  check "the standard bank contract is unchanged"
    (fmap payloadKind (payloadOfSize 0x1dc98) == Just RecipeBankD3
      && publicationCount == 223 && setupCount == 131 && activationCount == 92 && reviewedAllocationSize == 0x1dc98)
  check "the standard bank selects the base allowlist variant"
    (payloadBankVariant bankD3Payload == Just standardBankVariant)
  check "nonbank payloads select no bank allowlist"
    (all ((== Nothing) . payloadBankVariant)
      [classicChromePayload, classicChromeIconPayload, usbTogglePayload, afRingPayload, experimentPayload "synthetic"])
  standardRelinker <- runfilesRelinker bankD3Payload
  check "the standard bank retains its Rust archive"
    (fmap (takeFileName . rustBankArchive) (relinkerRust standardRelinker) == Just "bank.a")
  let standard = manifest { manifestEntries = payloadEntries bankD3Payload (Address base) }
      parse candidate = parseRamPushManifest "/synthetic/ram-push-manifest.json" (manifestDocument candidate) (runtimeBytes candidate)
      refusesManifest label candidate = do
        result <- try (parse candidate) :: IO (Either SomeException RamPushManifest)
        check label (case result of Left _ -> True; Right _ -> False)
      matchingContract candidate = ReviewedContract (sourceIdentity candidate) (publicationContractDigest (manifestPublications candidate))
      variants =
        [ (bankModal75Payload, modalScopedBankVariant, RecipeBankModal75, 0x1dcb8, 224, "build-recipe-bank-modal-ram.sh", "bank-modal.a")
        , (bankLifecyclePayload, lifecycleBankVariant, RecipeBankLifecycle, 0x1dcd8, 226, "build-recipe-bank-lifecycle-ram.sh", "bank-lifecycle.a")
        , (bankModalLifecyclePayload, modalLifecycleBankVariant, RecipeBankModalLifecycle, 0x1dcf8, 227, "build-recipe-bank-modal-lifecycle-ram.sh", "bank-modal-lifecycle.a")
        , (bankModalEnginePayload, modalEngineBankVariant, RecipeBankModal75MenuEngine, 0x3dd00, 225, "build-recipe-bank-modal-engine-ram.sh", "bank-modal.a")
        , (bankModalValuePayload, modalValueBankVariant, RecipeBankModal75MenuEngine, 0x33d00, 226, "build-recipe-bank-modal-value-ram.sh", "bank-modal.a")
        , (bankModalRenderPayload, modalRenderBankVariant, RecipeBankModal75MenuEngine, 0x3dd00, 228, "build-recipe-bank-modal-render-ram.sh", "bank-modal.a") ]
  _ <- parse standard
  let renderRuntime = B.replicate 0x3dd00 0
      renderPublications = publications ++
        [Publication index Activation (Address (0x00630000 + 4 * fromIntegral index))
          (WordValue 0xe92d4ff0) (WordValue 0xea000010) Both | index <- [223 .. 227]]
      renderManifest = standard
        { allocationSize = 0x3dd00, runtimeBytes = renderRuntime, runtimeDigest = sha256 renderRuntime
        , manifestPublications = renderPublications, manifestBankVariant = Just modalRenderBankVariant }
  renderParsed <- parse renderManifest
  check "render manifest retains explicit identity in the shared engine allocation"
    (manifestBankVariant renderParsed == Just modalRenderBankVariant)
  refusesManifest "render manifest refuses a tag with the engine count"
    (renderManifest { manifestPublications = take 225 renderPublications })
  refusesManifest "untagged render-sized vector cannot masquerade as modal lifecycle"
    (renderManifest { manifestBankVariant = Just modalLifecycleBankVariant })
  let renderState = RamPushState (manifestDigest renderParsed) (runtimeDigest renderParsed) (Address base) 0x3dd00
        (manifestEntries renderParsed) renderPublications callbackStockWord bootstrapBytes 1 (Just modalRenderBankVariant)
  renderStateDirectory <- pure (root </> "render-state")
  makeDir renderStateDirectory
  renderStatePath <- writeStateFile renderStateDirectory renderState
  readRamPushState renderStatePath >>= check "render state retains its explicit identity and 228 publications" . (== renderState)
  let valueRuntime = B.replicate 0x33d00 0
      valuePublications = take 226 renderPublications
      valueManifest = standard
        { allocationSize = 0x33d00, runtimeBytes = valueRuntime, runtimeDigest = sha256 valueRuntime
        , manifestPublications = valuePublications, manifestBankVariant = Just modalValueBankVariant }
  valueParsed <- parse valueManifest
  check "value manifest retains its explicit identity and 226 publications"
    (manifestBankVariant valueParsed == Just modalValueBankVariant)
  let valueState = RamPushState (manifestDigest valueParsed) (runtimeDigest valueParsed) (Address base) 0x33d00
        (manifestEntries valueParsed) valuePublications callbackStockWord bootstrapBytes 1 (Just modalValueBankVariant)
  let valueStateDirectory = root </> "value-state"
  makeDir valueStateDirectory
  valueStatePath <- writeStateFile valueStateDirectory valueState
  readRamPushState valueStatePath >>= check "value state preserves the identity needed by unload" . (== valueState)
  let fullIdentity = [("synthetic-source-" ++ show index, replicate 64 'a') | index <- [1..128 :: Int]]
  fullIdentityManifest <- parse (standard { sourceIdentity = fullIdentity })
  check "manifest accepts the complete 128-entry source identity"
    (length (sourceIdentity fullIdentityManifest) == 128)
  tooMany <- try (parse (standard { sourceIdentity = ("synthetic-extra", replicate 64 'a') : fullIdentity })) :: IO (Either SomeException RamPushManifest)
  check "129 source identities report the count bound"
    (case tooMany of Left failure -> "source_identity exceeds 128 entries" `isInfixOf` show failure; Right _ -> False)
  emptyIdentity <- try (parse (standard {sourceIdentity = []})) :: IO (Either SomeException RamPushManifest)
  check "empty source identity reports emptiness"
    (case emptyIdentity of Left failure -> "source_identity must be nonempty" `isInfixOf` show failure; Right _ -> False)
  validateReviewedContract (matchingContract standard) standard
  let withIdentityEntry entry = standard { sourceIdentity = entry : sourceIdentity standard }
  composedAccepted <- try (validateReviewedContract (matchingContract standard) (withIdentityEntry ("bank_allowlist_composed_sha256", replicate 64 'c'))) :: IO (Either SomeException ())
  check "the contract does not pin the composed bank allowlist digest"
    (case composedAccepted of Right () -> True; Left _ -> False)
  unpinnedRefused <- try (validateReviewedContract (matchingContract standard) (withIdentityEntry ("menu/unpinned_source.c", replicate 64 'c'))) :: IO (Either SomeException ())
  check "the contract refuses a source identity it does not pin"
    (case unpinnedRefused of Left _ -> True; Right () -> False)
  forM_ variants $ \(selected, variant, kind, size, count, script, archive) -> do
    let label = show kind ++ "-" ++ bankVariantTag variant
        checkVariant suffix = check (label ++ ": " ++ suffix)
    checkVariant "has a distinct allocation size and kind"
      (fmap payloadKind (payloadOfSize size) == Just kind
        && ramAllocArguments [show size] == Just (size, Nothing))
    checkVariant "publishes the selected activation group"
      (payloadSetupCount selected == 131 && payloadActivationCount selected == count - 131
        && payloadPublicationCount selected == count && payloadAbiVersion selected == 5)
    checkVariant "allocation matches the builder layout"
      (payloadAllocationSize selected == size && size == fromIntegral (runtimeAllocationSizeFor variant))
    checkVariant "selects its publication group's allowlist composition"
      (payloadBankVariant selected == Just variant)
    checkVariant "keeps the bank installer placement"
      (all (\at -> payloadInstallerStart selected at == runtimeInstallerBase at) alignedBases)
    relinker <- runfilesRelinker selected
    checkVariant "selects its own script and Rust archive"
      (takeFileName (relinkerScript relinker) == script
        && fmap (takeFileName . rustBankArchive) (relinkerRust relinker) == Just archive)
    let selectedRuntime = B.replicate size 0
        selectedPublications = publications ++
          [Publication index Activation (Address (0x00630000 + 4 * fromIntegral index))
            (WordValue 0xe1000000) (WordValue 0xea000010) Both | index <- [223..count-1]]
        candidate = manifest
          { allocationSize = size, runtimeBytes = selectedRuntime, runtimeDigest = sha256 selectedRuntime
          , manifestEntries = payloadEntries selected (Address base), manifestPublications = selectedPublications
          , manifestBankVariant = Just variant }
        selectedAllocation = allocation { allocSize = size }
        selectedState = RamPushState (manifestDigest candidate) (runtimeDigest candidate) (Address base) size
          (manifestEntries candidate) selectedPublications callbackStockWord bootstrapBytes 1 (Just variant)
    parsed <- parse candidate
    checkVariant "manifest retains its size and publication contract"
      (allocationSize parsed == size && manifestPublications parsed == selectedPublications)
    refusesManifest (label ++ ": refuses the normal publication list") (candidate { manifestPublications = publications })
    refusesManifest (label ++ ": cannot replace the normal publication list") (standard { manifestPublications = selectedPublications })
    refusesManifest (label ++ ": refuses a default-sized runtime") (candidate { runtimeBytes = runtime, runtimeDigest = sha256 runtime })
    reviewed <- try (validateReviewedContract (matchingContract candidate) candidate) :: IO (Either SomeException ())
    checkVariant "matching identities and publication digest do not grant review"
      (case reviewed of Left _ -> True; Right _ -> False)
    let stateDirectory = root </> (label ++ "-state")
    makeDir stateDirectory
    selectedStatePath <- writeStateFile stateDirectory selectedState
    readRamPushState selectedStatePath >>= checkVariant "state retains its size and publications" . (== selectedState)
    let invalidStateDirectory = root </> (label ++ "-state-invalid")
    makeDir invalidStateDirectory
    invalidStatePath <- writeStateFile invalidStateDirectory (selectedState { statePublications = publications })
    invalidState <- try (readRamPushState invalidStatePath) :: IO (Either SomeException RamPushState)
    checkVariant "state refuses the normal publication count" (case invalidState of Left _ -> True; Right _ -> False)
    liveModel <- newModel Nothing
    let liveDirectory = root </> (label ++ "-live")
        selectedRetained = RetainedAllocation (liveDirectory </> "ram-alloc-state.json") selectedAllocation
        clock = Clock (pure 0)
        api = modelApi liveModel
    makeDir liveDirectory
    liveAlloc <- runRamAlloc size clock api liveDirectory
    checkVariant "live allocation refuses" (case liveAlloc of RamAllocRefused _ -> True; _ -> False)
    livePush <- runRamPush (matchingContract candidate) selectedRetained candidate clock api liveDirectory
    checkVariant "live push refuses" (case livePush of RamPushRefused _ -> True; _ -> False)
    liveFree <- runRamFree selectedRetained clock api liveDirectory
    checkVariant "live free refuses" (case liveFree of RamFreeRefused _ -> True; _ -> False)
    liveUnload <- runRamUnload (matchingContract candidate) selectedState clock api liveDirectory
    checkVariant "live unload refuses" (case liveUnload of RamUnloadRefused _ -> True; _ -> False)
    liveRelink <- try (runRamRelink (matchingContract candidate) selectedRetained liveDirectory) :: IO (Either SomeException FilePath)
    checkVariant "live relink refuses" (case liveRelink of Left _ -> True; Right _ -> False)
    readIORef (modelUsbCalls liveModel) >>= checkVariant "live refusals make no USB calls" . (== 0)
  requireReviewedBytes (C.pack "manifest") (C.pack "runtime") (C.pack "manifest") (C.pack "runtime")
  changedRuntime <- try (requireReviewedBytes (C.pack "manifest") (C.pack "runtime") (C.pack "manifest") (C.pack "changed"))
    :: IO (Either SomeException ())
  check "reviewed build rejects byte mismatch" (case changedRuntime of Left _ -> True; Right _ -> False)
  unreviewed <- try (validateReviewedContract contract manifest) :: IO (Either SomeException ())
  check "compiled contract rejects an unreviewed manifest before USB" (case unreviewed of Left _ -> True; Right _ -> False)

  let allocateRetained label model = do
        let directory = root </> (label++"-alloc")
        makeDir directory
        outcome <- runAlloc directory model
        path <- case outcome of
          RamAllocated statePath -> pure statePath
          other -> error (label++" allocation failed: "++show other)
        readRetainedAllocation path
      allocate label model = retainedAllocation <$> allocateRetained label model

  untrustedModel <- newModel Nothing
  let untrustedDir = root </> "untrusted-manifest"
  makeDir untrustedDir
  untrustedOutcome <- runRamPush contract (RetainedAllocation (untrustedDir </> "ram-alloc-state.json") allocation) manifest (Clock (pure 0)) (modelApi untrustedModel) untrustedDir
  check "production ram-push rejects an unreviewed manifest" (case untrustedOutcome of RamPushRefused _ -> True; _ -> False)
  readIORef (modelHostWrites untrustedModel) >>= check "unreviewed manifest is refused before USB writes" . null

  let withIdentity identity = manifest { sourceIdentity = identity }
      substituteTool name = [(entry, if entry == name then replicate 64 'f' else digest) | (entry,digest) <- reviewedSourceIdentity contract]
      refusalOf label candidate = do
        model <- newModel Nothing
        let directory = root </> ("tool-identity-"++label)
        makeDir directory
        usbBefore <- readIORef (modelUsbCalls model)
        outcome <- runRamPush contract (RetainedAllocation (directory </> "ram-alloc-state.json") allocation) candidate (Clock (pure 0)) (modelApi model) directory
        usbAfter <- readIORef (modelUsbCalls model)
        check "tool identity refusals make no USB call" (usbAfter == usbBefore)
        pure (case outcome of RamPushRefused reason -> reason; other -> error ("expected a refusal: "++show other))
  reviewedIdentityRefusal <- refusalOf "reviewed" (withIdentity (reviewedSourceIdentity contract))
  check "reviewed tool identities pass the source identity check" (not ("source identity" `isInfixOf` reviewedIdentityRefusal))
  forM_ ["build-recipe-bank","ram-push-toolchain"] $ \tool -> do
    substitutedRefusal <- refusalOf (tool++"-substituted") (withIdentity (substituteTool tool))
    check (tool++" with a substituted identity is refused") ("source identity" `isInfixOf` substitutedRefusal)
    staleRefusal <- refusalOf (tool++"-missing") (withIdentity (filter ((/= tool) . fst) (reviewedSourceIdentity contract)))
    check (tool++" missing from a stale builder's manifest is refused") ("source identity" `isInfixOf` staleRefusal)

  noPeer <- newModel Nothing
  let noPeerDir = root </> "no-peer"
  makeDir noPeerDir
  noPeerOutcome <- runRamPushWith PeerCacheUnbound NoRamPushObserver allocation manifest (Clock (pure 0)) (modelApi noPeer) noPeerDir
  check "unbound peer refuses before USB and writes" (case noPeerOutcome of RamPushRefused _ -> True; _ -> False)
  readIORef (modelHostWrites noPeer) >>= check "unbound peer has no writes" . null
  parkModel <- newModel Nothing
  parkAllocation <- allocate "park-timeout" parkModel
  writeIORef (modelInstallStatus parkModel) 0xfffffff9
  let parkDir = root </> "park-timeout"
  makeDir parkDir
  parkOutcome <- runPush parkDir parkModel parkAllocation
  check "CPU1 park timeout refuses install" (case parkOutcome of RamPushRefused _ -> True; _ -> False)
  readIORef (modelInternalWrites parkModel) >>= check "CPU1 park timeout precedes every publication write" . null
  let spinReportOutcome label spins = do
        model <- newModel Nothing
        retainedSpin <- allocate label model
        writeIORef (modelSpinReport model) spins
        let directory = root </> label
        makeDir directory
        runPush directory model retainedSpin
  constantBound <- spinReportOutcome "constant-bound" (0x10010000,0x10010000,0)
  check "install reporting a constant in place of the spin bound is uncertain" (case constantBound of RamPushUncertain _ -> True; _ -> False)
  exhaustedAck <- spinReportOutcome "exhausted-ack" (0x80000,0x80000,17)
  check "install whose ACK wait reached the spin bound is uncertain" (case exhaustedAck of RamPushUncertain _ -> True; _ -> False)
  exhaustedPark <- spinReportOutcome "exhausted-park" (0x80000,3,0x80000)
  check "install whose PARKED wait reached the spin bound is uncertain" (case exhaustedPark of RamPushUncertain _ -> True; _ -> False)

  rehearseRamAlloc reviewedAllocationSize
  rehearseRamPush allocation manifest

  mismatchModel <- newModel Nothing
  let mismatchDir = root </> "allocation-mismatch"
      mismatch = allocation { allocBase = Address (base+32) }
  makeDir mismatchDir
  mismatchOutcome <- runPush mismatchDir mismatchModel mismatch
  check "allocation base mismatch refuses" (case mismatchOutcome of RamPushRefused _ -> True; _ -> False)
  readIORef (modelHostWrites mismatchModel) >>= check "allocation mismatch has no writes" . null

  let refusal label mutate = do
        model <- newModel Nothing
        retained <- allocate label model
        writesBefore <- length <$> readIORef (modelHostWrites model)
        _ <- mutate model
        let directory = root </> label
        makeDir directory
        outcome <- runPush directory model retained
        check (label++" refuses") (case outcome of RamPushRefused _ -> True; _ -> False)
        writesAfter <- length <$> readIORef (modelHostWrites model)
        check (label++" has no push RAM write") (writesAfter == writesBefore)
  refusal "table-pointer" $ \model -> writeWord (modelMemory model) (unAddress callbackTablePointerAddress) 0
  refusal "slot" $ \model -> writeWord (modelMemory model) (unAddress callbackSlotAddress) 0
  refusal "old-word" $ \model -> writeWord (modelMemory model) (unAddress (publicationAddress (publications!!47))) 0
  refusal "allocation-guard" $ \model -> writeBytes (modelMemory model) base [1]
  refusal "identity" $ \model -> modifyIORef' (modelIdentity model) (\bytes -> take 2 bytes ++ [0] ++ drop 3 bytes)
  refusal "f7" $ \model -> modifyIORef' (modelConfig model) (\bytes -> take (36+0xf7) bytes ++ [0] ++ drop (37+0xf7) bytes)

  freeModel <- newModel Nothing
  retainedForFree <- allocateRetained "free" freeModel
  rehearseRamFree (retainedAllocation retainedForFree)
  let freeDir = root </> "free"
  makeDir freeDir
  freed <- runRamFree retainedForFree (Clock (pure 0)) (modelApi freeModel) freeDir
  check "happy free" (freed == RamFreed)
  readIORef (modelFreed freeModel) >>= check "ram-free calls stock free"
  let marker = consumedMarkerPath retainedForFree
  check "consumed marker sits beside the allocation state" (takeDirectory marker == takeDirectory (retainedStatePath retainedForFree))
  markerMode <- fileMode <$> getFileStatus marker
  check "consumed marker is mode 0600" (markerMode `intersectFileModes` 0o777 == 0o600)
  markerBytes <- B.readFile marker
  check "consumed marker records the freed base and allocation hash"
    (C.pack "0x00a80000" `B.isInfixOf` markerBytes && C.pack (allocGuard (retainedAllocation retainedForFree)) `B.isInfixOf` markerBytes)
  writesAfterFree <- length <$> readIORef (modelHostWrites freeModel)
  let tagGuardDir = root </> "second-free-tag-guard"
  makeDir tagGuardDir
  tagGuarded <- runRamFreeWith PeerCacheBound (retainedAllocation retainedForFree) (Clock (pure 0)) (modelApi freeModel) tagGuardDir
  check "camera-side freed tag refuses a second free" (case tagGuarded of RamFreeRefused _ -> True; _ -> False)
  readIORef (modelHostWrites freeModel) >>= check "tag-guard refusal sends no write" . (== writesAfterFree) . length
  writeBytes (modelMemory freeModel) base (replicate 8 0)
  writeIORef (modelFreed freeModel) False
  restored <- B.pack <$> readBytes (modelMemory freeModel) base reviewedAllocationSize
  check "camera-side tag is gone and the allocation hash is back to the state's" (sha256 restored == allocGuard (retainedAllocation retainedForFree))
  usbBeforeConsumed <- readIORef (modelUsbCalls freeModel)
  let secondFreeDir = root </> "second-free"
  makeDir secondFreeDir
  secondFree <- runRamFree retainedForFree (Clock (pure 0)) (modelApi freeModel) secondFreeDir
  check "second ram-free from a consumed state is refused with the camera tag gone" (case secondFree of RamFreeRefused _ -> True; _ -> False)
  readIORef (modelFreed freeModel) >>= check "refused second free does not free" . not
  let freedPushDir = root </> "push-after-free"
  makeDir freedPushDir
  freedPush <- runRamPush contract retainedForFree manifest (Clock (pure 0)) (modelApi freeModel) freedPushDir
  check "ram-push from a consumed state is refused with the camera tag gone" (case freedPush of RamPushRefused _ -> True; _ -> False)
  readIORef (modelUsbCalls freeModel) >>= check "consumed-state refusals make no USB call" . (== usbBeforeConsumed)

  observerModel <- newModel Nothing
  observerAllocation <- allocate "before-publication" observerModel
  (observerApi, observerQueries) <- observeRamQueries observerModel
  observerCalls <- newIORef (0 :: Int)
  let observerDir = root </> "before-publication"
      observe = do
        modifyIORef' observerCalls (+1)
        bytes <- B.pack <$> readBytes (modelMemory observerModel) base (B.length runtime)
        check "before-publication sees the complete uploaded runtime" (bytes == runtime)
        observerQueries >>= check "before-publication sees completed synchronization of the uploaded runtime" . (== uploadedSynchronization)
        checkUnpublished "before-publication" observerModel
  makeDir observerDir
  observedOutcome <- runRamPushWith PeerCacheBound (BeforeRamPublication observe) observerAllocation manifest (Clock (pure 0)) observerApi observerDir
  observedStatePath <- case observedOutcome of
    RamPushInstalled path -> pure path
    other -> error ("observed push failed: " ++ show other)
  observedState <- readRamPushState observedStatePath
  check "observed push retains the manifest publication contract" (statePublications observedState == publications)
  readIORef observerCalls >>= check "before-publication runs exactly once" . (== 1)
  observerQueries >>= check "installer is invoked once after the observation boundary" .
    (== uploadedSynchronization ++ [RamQueryObservation (installEntry (manifestEntries manifest)) RuntimeUploaded])
  forM_ publications $ \publication -> do
    actual <- u32 0 <$> readBytes (modelMemory observerModel) (unAddress (publicationAddress publication)) 4
    check "observed push publishes every new word" (actual == unWordValue (publicationNew publication))

  observerFailureModel <- newModel Nothing
  observerFailureAllocation <- allocate "before-publication-failure" observerFailureModel
  (observerFailureApi, observerFailureQueries) <- observeRamQueries observerFailureModel
  observerFailureCalls <- newIORef (0 :: Int)
  let observerFailureDir = root </> "before-publication-failure"
      observerError = userError "before-publication observer failed"
      failObservation = do
        modifyIORef' observerFailureCalls (+1)
        observerFailureQueries >>= check "failing observer starts after uploaded runtime synchronization" . (== uploadedSynchronization)
        checkUnpublished "failing observer boundary" observerFailureModel
        throwIO observerError
  makeDir observerFailureDir
  observerFailed <- runRamPushWith PeerCacheBound (BeforeRamPublication failObservation) observerFailureAllocation manifest (Clock (pure 0)) observerFailureApi observerFailureDir
  check "observer exception is preserved as an uncertain uploaded allocation"
    (case observerFailed of RamPushUncertain _ -> True; _ -> False)
  readIORef observerFailureCalls >>= check "failing observer is not retried" . (== 1)
  observerFailureQueries >>= check "observer exception sends no installer invocation" . (== uploadedSynchronization)
  checkUnpublished "after observer exception" observerFailureModel
  failureRuntime <- B.pack <$> readBytes (modelMemory observerFailureModel) base (B.length runtime)
  check "observer exception retains the uploaded runtime" (failureRuntime == runtime)
  failureSlot <- u32 0 <$> readBytes (modelMemory observerFailureModel) (unAddress callbackSlotAddress) 4
  check "observer exception leaves the stock callback selected" (failureSlot == unWordValue callbackStockWord)
  doesFileExist (observerFailureDir </> "ram-push-state.json") >>= check "observer exception writes no installed state" . not

  observerRefusalModel <- newModel Nothing
  observerRefusalCalls <- newIORef (0 :: Int)
  case publications of
    first : _ -> writeWord (modelMemory observerRefusalModel) (unAddress (publicationAddress first)) 0
    [] -> fail "observer fixture has no publications"
  let observerRefusalDir = root </> "before-publication-preflight-refusal"
  makeDir observerRefusalDir
  observerRefused <- runRamPushWith PeerCacheBound (BeforeRamPublication (modifyIORef' observerRefusalCalls (+1))) allocation manifest (Clock (pure 0)) (modelApi observerRefusalModel) observerRefusalDir
  check "observer does not bypass old-word preflight refusal" (case observerRefused of RamPushRefused _ -> True; _ -> False)
  readIORef observerRefusalCalls >>= check "preflight refusal never invokes the observer" . (== 0)
  readIORef (modelHostWrites observerRefusalModel) >>= check "observer preflight refusal sends no RAM write" . null

  happyModel <- newModel Nothing
  retained <- allocate "happy" happyModel
  let happyDir = root </> "happy"
  makeDir happyDir
  happy <- runPush happyDir happyModel retained
  statePath <- case happy of
    RamPushInstalled path -> pure path
    other -> error ("happy push failed: "++show other)
  state <- readRamPushState statePath
  check "installed state retains allocation extent" (stateSize state == allocSize retained)

  requireReviewedState state manifest
  let entries = stateEntries state
      tamperedEntry = state { stateEntries = entries { rollbackEntry = Address (unAddress (rollbackEntry entries)+4) } }
      tamperPublication publication
        | publicationIndex publication == 0 =
            publication { publicationNew = WordValue (unWordValue (publicationNew publication)+1) }
        | otherwise = publication
      tamperedPublication = state { statePublications = map tamperPublication (statePublications state) }
      tamperedHash = state { stateRuntimeDigest = replicate 64 '0' }
      rejectsState candidate = do
        outcome <- try (requireReviewedState candidate manifest) :: IO (Either SomeException ())
        pure (case outcome of Left _ -> True; Right _ -> False)
  rejectsState tamperedEntry >>= check "unload authentication rejects tampered entry"
  rejectsState tamperedPublication >>= check "unload authentication rejects tampered publication"
  rejectsState tamperedHash >>= check "unload authentication rejects tampered runtime hash"
  forM_ publications $ \publication -> do
    actual <- u32 0 <$> readBytes (modelMemory happyModel) (unAddress (publicationAddress publication)) 4
    check "installed publication" (actual == unWordValue (publicationNew publication))
  hostWrites <- readIORef (modelHostWrites happyModel)
  let allowed (address,bytes) =
        (unAddress bootstrapAddress <= address && address + fromIntegral (length bytes) <= unAddress bootstrapAddress + 32)
        || (address == unAddress callbackSlotAddress && length bytes == 4)
        || (address == unAddress queryOutputAddress + 4 && length bytes == 4)
        || (address == unAddress queryOutputAddress && length bytes == 8)
        || (base <= address && address + fromIntegral (length bytes) <= base + fromIntegral (allocationSize manifest))
  check "host writes stay in runtime, bootstrap protocol scratch or slot" (all allowed hostWrites)
  internal <- readIORef (modelInternalWrites happyModel)
  check "installer writes exactly the manifest destinations" (map fst internal == map (unAddress . publicationAddress) publications)
  rehearseRamUnload state

  let staleFreeDir = root </> "stale-free"
  makeDir staleFreeDir
  staleFree <- runRamFreeWith PeerCacheBound retained (Clock (pure 0)) (modelApi happyModel) staleFreeDir
  check ("ram-free refuses an allocation modified by install: "++show staleFree) (case staleFree of RamFreeRefused _ -> True; _ -> False)
  readIORef (modelFreed happyModel) >>= check "stale ram-free does not free" . not

  refusalUnloadModel <- newModel Nothing
  forM_ publications $ \publication ->
    writeWord (modelMemory refusalUnloadModel) (unAddress (publicationAddress publication)) (unWordValue (publicationNew publication))
  writeWord (modelMemory refusalUnloadModel) (unAddress (publicationAddress (publications!!31))) 0
  let refusalUnloadDir = root </> "unload-preflight"
  makeDir refusalUnloadDir
  refusedUnload <- runRamUnloadWith PeerCacheBound state (Clock (pure 0)) (modelApi refusalUnloadModel) refusalUnloadDir
  check "unload active-word mismatch refuses" (case refusedUnload of RamUnloadRefused _ -> True; _ -> False)
  readIORef (modelHostWrites refusalUnloadModel) >>= check "unload refusal has no RAM write" . null
  readIORef (modelInternalWrites refusalUnloadModel) >>= check "unload refusal has no target write" . null

  let unloadDir = root </> "unload"
  makeDir unloadDir
  unloaded <- runRamUnloadWith PeerCacheBound state (Clock (pure 0)) (modelApi happyModel) unloadDir
  check ("happy unload: "++show unloaded) (unloaded == RamUnloaded)
  readIORef (modelFreed happyModel) >>= check "unload retains allocation" . not
  forM_ publications $ \publication -> do
    actual <- u32 0 <$> readBytes (modelMemory happyModel) (unAddress (publicationAddress publication)) 4
    check "rolled-back publication" (actual == unWordValue (publicationOld publication))
  allInternal <- readIORef (modelInternalWrites happyModel)
  check "rollback is reverse order" (map fst (drop publicationCount allInternal) == reverse (map (unAddress . publicationAddress) publications))

  let callbackFailure label failureOffsets expected = do
        model <- newModel Nothing
        retainedState <- allocateRetained label model
        ordinal <- readIORef (modelWriteReplies model)
        writeIORef (modelWriteFailures model) (map (ordinal+) failureOffsets)
        let directory = root </> label
        makeDir directory
        outcome <- runRamFree retainedState (Clock (pure 0)) (modelApi model) directory
        message <- case outcome of
          RamFreeUncertain reason -> pure reason
          other -> error (label++" expected uncertain outcome: "++show other)
        check (label++" reports callback-slot state") (expected `isInfixOf` message)
        current <- u32 0 <$> readBytes (modelMemory model) (unAddress callbackSlotAddress) 4
        check (label++" cleanup writes stock callback") (current == unWordValue callbackStockWord)
        doesFileExist (consumedMarkerPath retainedState) >>= check (label++" uncertain free writes the consumed marker")
  callbackFailure "callback-restored" [2] "stock callback slot restored"
  callbackFailure "callback-restore-uncertain" [2,3] "do not use camera; power it off now"

  failedModel <- newModel (Just 13)
  failedAllocation <- allocate "mid-sequence" failedModel
  let failedDir = root </> "mid-sequence"
  makeDir failedDir
  failed <- runPush failedDir failedModel failedAllocation
  check "mid-sequence loss is uncertain" (case failed of RamPushUncertain _ -> True; _ -> False)
  journal <- readFile (failedDir </> "journal.v1")
  check "mid-sequence state is recorded" ("allocation-active=True" `isInfixOf` journal)
  sent <- readIORef (modelHostWrites failedModel)
  check "lost write is not retried" (length sent == 14)

  -- The RAM caps count one native-outcome line per completed transfer.
  happyJournal <- lines <$> readFile (happyDir </> "journal.v1")
  let kindCount :: String -> Int
      kindCount kind = length (filter (isInfixOf (show kind)) happyJournal)
  check "every completed transfer journals its native outcome"
    (kindCount "native-outcome" > 0 && kindCount "native-outcome" == kindCount "reply-before-deadline-check")

  removeDirectoryRecursive root
  putStrLn "ram-push tests: ok"
