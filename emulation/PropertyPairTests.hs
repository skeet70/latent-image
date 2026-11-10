{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.KeyMap as KM
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft)
import Data.List (find)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import Emulation.LocalUiMenu (parseLocalUiMenu)
import Emulation.PropertyPairCheck
import Emulation.PropertyPairPlan hiding (image4Base)
import Emulation.PropertyPairRoutes
import Emulation.Sha256 (sha256)
import EmulatorUsb (CrawlKey (..))
import PropertyPairFixture
import System.Exit (exitFailure)

main :: IO ()
main = do
  let edges =
        [ CrawlEdge 3 [KeyOk, KeyDown, KeyOk] [SetterEffect 4, CommitEffect]
        , CrawlEdge 7 [KeyOk, KeyOk] [SetterEffect 4, CommitEffect]
        , CrawlEdge 9 [KeyUp, KeyOk] [SetterEffect 4, CommitEffect]
        , CrawlEdge 1 [KeyOk] []
        , CrawlEdge 12 [KeyDown] [CommitEffect, SetterEffect 4]
        , CrawlEdge 15 [KeyBack, KeyBack, KeyOk] [SetterEffect 5]
        , CrawlEdge 18 [KeyPlay, KeyOk] [SetterEffect 6]
        , CrawlEdge 19 [KeyOk, KeyPlay] [SetterEffect 7]
        , CrawlEdge 21 [KeyRight] [SetterEffect 0x1c9, SetterEffect 0x1c8, SetterEffect 0x27f, SetterEffect 0x280]
        , CrawlEdge 22 [KeyLeft] [SetterEffect 0x1c9, SetterEffect 0x278]
        ]
      (selected, playbackTrials, timeDrivenTrials) = selectCrawlRoutes edges
      file source routes = RouteFile (replicate 64 '0') "test" source [Exclusion "reason" playbackTrials] routes
      roundTrip source routes = decodeRouteFile (BL.toStrict (encodeRouteFile (file source routes)))
      retainedSource = RetainedObservationSource (sha256 "receipt") (sha256 "steps") (sha256 "hits")
      revisedSource = SourceRevisedObservationSource (sha256 "receipt") (sha256 "steps") (sha256 "hits") (sha256 "source binding")
      valid = phaseRoutes ++ selected ++ declaredRoutes
      ownedWith hits = ownedStep { stepHits = hits }
      wide = 0x9c281c
      wideStep = retainedStep
        { stepRows = stepRows retainedStep ++ [cpuWrite wide 16 Nothing (replicate 16 0) setterCall, cpuWrite (wide + 3) 1 (Just 0x55) [0x77] setterCall]
        , stepAfter = poke ([(wide + offset, 0x11) | offset <- [0 .. 15]] ++ [(wide + 3, 0x55)]) (stepAfter retainedStep) }
      transferred = poke [(0x96cc80, 3)] (stepAfter retainedStep)
      guardHit address = ownedWith (stepHits ownedStep ++ [hit (Guard RetiredGetterArm) address [] stackPointer 0])
  results <- sequence
    [ check "one route per ordered effect sequence, fewest presses then lowest trial"
        (map routeKeys selected == [[KeyOk, KeyOk], [KeyDown], [KeyBack, KeyBack, KeyOk], [KeyOk, KeyPlay], [KeyLeft]])
    , check "composed publication binds the complete staged image4"
        (fmap (digestOf . inputLinked) composedInputs == Right (digestOf composedImage)
          && (composedInputs >>= roleImage4) == Right composedImage)
    , check "composed plans qualify both instances of a shared boundary and the custom boundary"
        (case composedInputs >>= derivePlan of
          Right plan -> all (\(accessor,address,_,index) -> M.lookup (address+fromIntegral (index*4)) (planStores plan) == Just accessor) composedBoundaries
            && classifyWriter plan (composedCodeStart+0x80) == CarrierBody
          Left _ -> False)
    , check "composed plans guard retired bank custom and record interiors but not live entries"
        (case composedInputs >>= derivePlan of
          Right plan -> all (\address -> M.lookup address (planSites plan) == Just (Guard RetiredStorage)) [0x61d8e0,0x6a8488,0x6ac2b8]
            && all (\(address,_) -> M.lookup address (planSites plan) == if address == area3Append then Just Area3Append else Nothing) composedEntries
          Left _ -> False)
    , check "composed plans reject a modified checked store encoding"
        (isLeft (do inputs <- composedInputs
                    let changed = putWord (composedCodeStart+4) 0 (inputLinked inputs)
                        contract = (inputContract inputs){contractLinkedSha256=digestOf changed,contractStagedImage4=digestOf changed}
                    derivePlan inputs{inputContract=contract,inputLinked=changed}))
    , check "composed plans reject narrowed custom halfword and word stores"
        (all (\(_,address,_,index) -> isLeft (do
          inputs <- composedInputs
          let changed = putWord (address+fromIntegral (4*index)) 0xe5c01000 (inputLinked inputs)
              contract = (inputContract inputs){contractLinkedSha256=digestOf changed,contractStagedImage4=digestOf changed}
          derivePlan inputs{inputContract=contract,inputLinked=changed}))
          [boundary | boundary@(accessor,_,_,_) <- composedBoundaries, accessor `elem` [CustomHalf,CustomWord]])
    , check "composed plans reject raw shim substitutions rather than falling back"
        (isLeft (do inputs <- composedInputs
                    let contract = inputContract inputs
                    derivePlan inputs{inputContract=contract{contractEntries=[("xm1_bank_store8",composedCodeStart)]}}))
    , check "guarded absolute stores resolve their actual destination and reject whole-write overflow"
        (storeEffect StorageByte [0x409c295c,0xab] == Just (0x409c295c,1,0xab)
          && storeEffect StorageHalf [0x96d1a9,0x1234] == Nothing
          && storeEffect CustomByte [0x89f86f,0x45] == Just (0x89f86f,1,0x45)
          && storeEffect CustomByte [0x89f870,0x45] == Nothing
          && storeEffect OutputByte [0x89ec00,40,39,0x67] == Just (0x89ec27,1,0x67)
          && storeEffect OutputByte [0x89ec00,40,40,0x67] == Nothing
          && storeEffect StatusWord [0xffffffc1] == Just (0x89edc4,4,0xffffffc1))
    , check "custom halfword writes retain exact aligned materialization windows"
        (storeEffect CustomHalf [0x89f838,0x1234] == Just (0x89f838,2,0x1234)
          && storeEffect CustomHalf [0x89f86a,0x1234] == Just (0x89f86a,2,0x1234)
          && storeEffect CustomHalf [0x89f874,0x1234] == Just (0x89f874,2,0x1234)
          && storeEffect CustomHalf [0x89f8a6,0x1234] == Just (0x89f8a6,2,0x1234)
          && storeEffect CustomHalf [0x409c295c,0x1234] == Just (0x409c295c,2,0x1234)
          && storeEffect CustomHalf [0x409c2982,0x1234] == Just (0x409c2982,2,0x1234)
          && all ((== Nothing) . storeEffect CustomHalf . (\address -> [address,0x1234]))
            [0x89f834,0x89f836,0x89f839,0x89f86c,0x89f86e,0x89f872,0x89f8a8,0x409c295a,0x409c2984])
    , check "custom word writes retain reset previous and handle boundaries only"
        (storeEffect CustomWord [0x89f834,1] == Just (0x89f834,4,1)
          && storeEffect CustomWord [0x89f838,0x12345678] == Just (0x89f838,4,0x12345678)
          && storeEffect CustomWord [0x89f86c,0x89f8f8] == Just (0x89f86c,4,0x89f8f8)
          && all ((== Nothing) . storeEffect CustomWord . (\address -> [address,0x12345678]))
            [0x89f830,0x89f835,0x89f86e,0x89f870,0x89f874,0x409c295c])
    , check "halfword output bounds the whole even-addressed write"
        (storeEffect OutputHalf [0x89ec00,40,38,0x1234] == Just (0x89ec26,2,0x1234)
          && storeEffect OutputHalf [0x89ec01,4,1,0x1234] == Just (0x89ec02,2,0x1234)
          && all ((== Nothing) . storeEffect OutputHalf)
            [ [0x89ec00,40,39,0x1234], [0x89ec00,40,1,0x1234]
            , [0x89ec01,40,0,0x1234], [0xfffffffe,2,0,0x1234]
            , [0x89ec00,40,0xffffffff,0x1234] ])
    , check "composed persistence keeps append wire payload read program and status in order"
        (case customPersistence [customAppend,customRead,customProgram,customProgramResult,customRead] of
          Right [CustomAppend {},CustomRead {},CustomProgram {},CustomProgramResult {},CustomRead {}] -> True
          _ -> False)
    , check "typed and retained append ABI boundaries normalize the same captured request"
        (customPersistence [customAppend,customProgram,customProgramResult]
          == customPersistence [customAppendAt area3Append [0x89f8f8,0,0x89f874],customProgram,customProgramResult])
    , check "an append count without captured logical wire is refused"
        (isLeft (customPersistence [hit Area3Append area3Append [0x89f8f8,0,0x89f874] stackPointer 0]))
    , check "a program count without the physical staging bytes is refused"
        (isLeft (customPersistence [customAppend,hit RecordProgram 0x6ab2f4 [0x89fa80,0,52,0x600] stackPointer 0,customProgramResult]))
    , check "custom physical programs require 0x600 rather than logical or bank record extents"
        (all (\count -> isLeft (customPersistence [customAppend,memoryHit RecordProgram 0x6ab2f4 [0x89fa80,0,52,count] [B.take (fromIntegral count) customProgramBytes],customProgramResult])) [52,0x1c2])
    , check "a program with changed append metadata is refused"
        (isLeft (customPersistence [customAppend,memoryHit RecordProgram 0x6ab2f4 [0x89fa80,0,52,0x600] [B.take 2 customProgramBytes <> B.singleton 0xee <> B.drop 3 customProgramBytes],customProgramResult]))
    , check "equal append counts do not hide physical tail changes"
        (let original = customPersistence [customAppend,customProgram,customProgramResult]
             changed = customPersistence [customAppend,memoryHit RecordProgram 0x6ab2f4 [0x89fa80,0,52,0x600] [B.take 100 customProgramBytes <> B.singleton 0xee <> B.drop 101 customProgramBytes],customProgramResult]
         in not (isLeft original) && not (isLeft changed) && original /= changed)
    , check "equal append counts do not hide medium error status"
        (let success = customPersistence [customAppend,customProgram,customProgramResult]
             failure = customPersistence [customAppend,customProgram,hit RecordProgramResult 0x6ab390 [0xffffffff] stackPointer 0]
         in not (isLeft success) && not (isLeft failure) && success /= failure)
    , check "a custom medium request requires its matching result"
        (isLeft (customPersistence [customAppend,customProgram])
          && isLeft (customPersistence [customProgramResult])
          && isLeft (customPersistence [customAppend,customProgram,hit RecordEraseResult 0x6ab4c0 [0] stackPointer 0]))
    , check "nonexecuted conditional returns are not medium results"
        (customPersistence [customAppend,customProgram,hitFlags 0x40000000 (hit RecordProgramResult 0x6ab314 [0xffffffff] stackPointer 0),customProgramResult]
          == customPersistence [customAppend,customProgram,customProgramResult]
          && persistenceStatusExecutes 0x6ab348 0x20000000
          && not (persistenceStatusExecutes 0x6ab348 0x60000000))
    , check "both cache destinations are diagnostic and read scheduling does not change persistence equivalence"
        (let stockReads =
               [ customRead
               , hit RecordRead 0x6aad94 [0xb20000,0x8c0080] stackPointer 0
               , customRead ]
             ownedReads = [hit RecordRead 0x6aad94 [0xb20000,0x8a0080] stackPointer 0]
         in comparedPersistence (customAppend:stockReads ++ [customProgram,customProgramResult])
              (customAppend:ownedReads ++ [customProgram,customProgramResult]) == Right ()
           && customPersistence stockReads /= customPersistence ownedReads)
    , check "read diagnostics reject a destination outside either real cache"
        (isLeft (customPersistence [hit RecordRead 0x6aad94 [0xb00000,0x8e0080] stackPointer 0]))
    , check "ignoring cache-read diagnostics still compares physical program tails and status"
        (let normal = [customAppend,customRead,customProgram,customProgramResult]
             tailChange = memoryHit RecordProgram 0x6ab2f4 [0x89fa80,0,52,0x600]
               [B.take 100 customProgramBytes <> B.singleton 0xee <> B.drop 101 customProgramBytes]
         in isLeft (comparedPersistence normal [customAppend,tailChange,customProgramResult])
           && isLeft (comparedPersistence normal [customAppend,customProgram,hit RecordProgramResult 0x6ab390 [0xffffffff] stackPointer 0]))
    , check "erase failures retain their typed event before the subsequent program"
        (case customPersistence [customAppend,hit RecordErase 0x6ab458 [1] stackPointer 0,hit RecordEraseResult 0x6ab4c0 [0xffffffff] stackPointer 0,customProgram,customProgramResult] of
          Right [CustomAppend {},CustomErase {},CustomEraseResult {},CustomProgram {},CustomProgramResult {}] -> True
          _ -> False)
    , check "an edge leaving the screen PLAY opened is left out and listed" (playbackTrials == [18])
    , check "an edge whose only effects are the time-driven block is left out and listed, one with another setter is kept" (timeDrivenTrials == [21])
    , check "an edge without a property effect is not a route" (all ((/= [KeyOk]) . routeKeys) selected)
    , check "a crawl route keeps its edge's effects" (map routeBody (take 1 selected) == [KeyRoute [KeyOk, KeyOk] (CrawlTrial 7 [SetterEffect 4, CommitEffect])])
    , check "route files round-trip with crawl, retained, and source-revised provenance"
        (and [roundTrip source valid == Right (file source valid) | source <- [CrawlEffectsSource, retainedSource, revisedSource]])
    , check "a route file without the attach phases first is refused" (isLeft (roundTrip CrawlEffectsSource (selected ++ phaseRoutes)))
    , check "a key route needs a key" (isLeft (decodeRouteFile (C.pack (concat
        [ "{\"schema\":1,\"crawl_graph_sha256\":\"0\",\"selection\":\"t\",\"effects_source\":{\"kind\":\"crawl\"},\"exclusions\":[],\"routes\":["
        , "{\"name\":\"boot\",\"phase\":\"first-start\"},{\"name\":\"start-menu\",\"phase\":\"live-view\"},"
        , "{\"name\":\"attach\",\"phase\":\"attached\"},{\"name\":\"x\",\"keys\":[],\"crawl_trial\":1,\"effects\":[{\"kind\":\"commit\"}]}]}" ]))))
    , check "repeated route names are refused" (isLeft (roundTrip CrawlEffectsSource (phaseRoutes ++ selected ++ take 1 selected)))
    , check "an effects source with a key its kind does not have is refused"
        (isLeft (A.eitherDecodeStrict' @EffectsSource (C.pack "{\"kind\":\"crawl\",\"receipt_sha256\":\"00\"}")))
    , check "a source revision requires a valid binding digest"
        (case A.toJSON revisedSource of
          Object fields -> all (isLeft . A.eitherDecodeStrict' @EffectsSource . BL.toStrict . A.encode . Object)
            [KM.delete "binding_sha256" fields, KM.insert "binding_sha256" (String "invalid") fields]
          _ -> False)
    , check "a crawl route without effects is refused from the crawl and admitted from a retained observation"
        (let silent = phaseRoutes ++ [Route crawlRoute (KeyRoute [KeyOk] (CrawlTrial 1 []))]
         in isLeft (roundTrip CrawlEffectsSource silent) && roundTrip retainedSource silent == Right (file retainedSource silent))

    , check "a retained observation's last step supplies each crawl route's effects"
        (fmap (map routeBody . rebasedEffectsRoutes) (retainedEffects (retainedSidecars RetainedRole "completed") [Route crawlRoute (KeyRoute [KeyOk] (CrawlTrial 1 [CommitEffect]))])
          == Right [KeyRoute [KeyOk] (CrawlTrial 1 [SetterEffect 5, CommitEffect])])
    , check "a re-based route with only the time-driven block is dropped and listed, one with empty effects and one with another setter are kept"
        (let routes =
               [ Route (RouteName "crawl-trial-00031") (KeyRoute [KeyOk] (CrawlTrial 31 [CommitEffect]))
               , Route (RouteName "crawl-trial-00032") (KeyRoute [KeyOk, KeyOk] (CrawlTrial 32 [CommitEffect]))
               , Route (RouteName "crawl-trial-00033") (KeyRoute [KeyOk, KeyOk, KeyOk] (CrawlTrial 33 [CommitEffect]))
               ]
             steps' =
               [ retainedStep { stepHits = blockHits }
               , retainedStep { stepHits = [getterIn, getterOut] }
               , retainedStep { stepHits = setterIn : blockHits }
               ]
             result = retainedEffects (retainedSidecarsWith RetainedRole "completed" steps') routes
         in fmap (\rebased -> (map routeName (rebasedEffectsRoutes rebased), rebasedEffectsTimeDrivenTrials rebased)) result
              == Right ([RouteName "crawl-trial-00032", RouteName "crawl-trial-00033"], [31]))
    , check "effects are refused from an owned or uncompleted observation, or one without a route's last step"
        (let routes = [Route crawlRoute (KeyRoute [KeyOk] (CrawlTrial 1 [CommitEffect]))]
             refused sidecars chosen = either Just (const Nothing) (retainedEffects sidecars chosen)
         in refused (retainedSidecars OwnedRole "completed") routes == Just (ObservationNotRetained OwnedRole)
              && refused (retainedSidecars RetainedRole "refused") routes == Just (ObservationNotCompleted StatusRefused)
              && refused (retainedSidecars RetainedRole "completed") [Route crawlRoute (KeyRoute [KeyDown] (CrawlTrial 1 [CommitEffect]))]
                   == Just (LastStepMissing crawlRoute))

    , check "the owned observation qualifies with its store classed as an accessor write and its calls returned"
        (fmap (\result -> (map effectWriter (resultWrites result), map callResult (resultSetters result), map callResult (resultGetters result))) (crawlResult OwnedRole ownedStep)
          == Just ([OwnedAccessor], [Just 0], [Just 0xab]))
    , check "retained and owned observations of the same effects compare equal" (compared retainedStep ownedStep == Right ())
    , check "equivalent spec JSON encoding preserves the retained-owned comparison"
        (let base = observation OwnedRole ownedStep
             spec = " \n" <> obsSpec base <> " \n"
             receipt = case A.eitherDecodeStrict' (obsReceipt base) of
               Right (Object fields) -> case KM.lookup "files" fields of
                 Just (Object files) -> BL.toStrict (A.encode (Object
                   (KM.insert "files" (Object (KM.insert "spec.json" (A.toJSON (digestOf spec)) files)) fields)))
                 _ -> error "test receipt files are not an object"
               _ -> error "test receipt is not an object"
             edited = base { obsSpec = spec, obsReceipt = receipt }
             comparedEncoding = do
               retained <- qualify bindingsAdmitted RetainedRole (observation RetainedRole retainedStep)
               owned <- qualify bindingsAdmitted OwnedRole edited
               () <$ compareRoles retained owned
         in comparedEncoding == Right ())

    , check "a wide write's unknown bytes come from the next preimage, then the after regions"
        (fmap (concatMap effectBytes . take 1 . drop 1 . resultWrites) (crawlResult RetainedRole wideStep)
          == Just [Just (if offset == 3 then 0x77 else 0x11) | offset <- [0 .. 15 :: Int]])
    , check "a preimage that differs from the replayed byte is refused"
        (refusal RetainedRole retainedStep { stepRows = [cpuWrite written 1 (Just 0xab) [1] retiredSetter] } == Just (ReplayMismatch (StepInterval 1) written))
    , check "a host write leaves its bytes unknown to the next preimage"
        (refusal RetainedRole retainedStep { stepRows = stepRows retainedStep ++ [hostWrite 0x96cc80 1, cpuWrite 0x96cc80 1 (Just 3) [9] retiredSetter], stepAfter = transferred } == Nothing
          && refusal RetainedRole retainedStep { stepRows = stepRows retainedStep ++ [cpuWrite 0x96cc80 1 (Just 3) [9] retiredSetter], stepAfter = transferred } == Just (ReplayMismatch (StepInterval 1) 0x96cc80))

    , check "an entry whose caller is not an armed return site is refused"
        (refusal OwnedRole (ownedWith (hit SetterEntry setterEntry [5, 1] stackPointer (setterCall + 8) : drop 1 (stepHits ownedStep))) == Just (UnarmedReturn 0))
    , check "a second open call at the same stack and return is refused"
        (refusal OwnedRole (ownedWith (setterIn : stepHits ownedStep)) == Just (DuplicateOpenCall 1))
    , check "entries and returns at the bit-30 alias pair like their physical spelling"
        (fmap (map callResult . resultSetters) (crawlResult OwnedRole (ownedWith
          [ hit SetterEntry (setterEntry + 0x40000000) [5, 1] stackPointer (setterCall + 0x40000004), commit, storeHit BankStore8 0x10 0xab
          , hit SetterReturn (setterCall + 0x40000004) [0] stackPointer 0 ])) == Just [Just 0])

    , check "a retired getter arm's last word is guarded at both spellings"
        (refusal OwnedRole (guardHit (retiredGetter + 0x3c)) == Just (GuardedFetch 6) && refusal OwnedRole (guardHit (retiredGetter + 0x4000003c)) == Just (GuardedFetch 6))
    , check "a carrier fill word after the linked code is guarded"
        (refusal OwnedRole (ownedWith (stepHits ownedStep ++ [hit (Guard CarrierFill) (carrierEnd - 4) [] stackPointer 0])) == Just (GuardedFetch 6))
    , check "a slot-field arm word is guarded at both spellings, up to the next live arm"
        (let slotHit address = refusal OwnedRole (ownedWith (stepHits ownedStep ++ [hit (Guard RetiredSetterArm) address [] stackPointer 0]))
         in all (\address -> slotHit address == Just (GuardedFetch 6)) [slotSetter + 0x3c, slotSetter + 0x4000003c]
              && refusal OwnedRole (guardHit (slotGetter + 0x4000003c)) == Just (GuardedFetch 6)
              && slotHit (slotSetter + 0x40) == Just (UnarmedSite 6))
    , check "a control on a slot-field property restores only its setter table word to the stock arm"
        (let contract = either error id (decodeContract contractBytes)
             controlled = do
               patch <- controlPatch contract 7
               roleImage4 (PlanInputs ControlRole contract images linked (Just (controlWord patch)) Nothing)
             wordAt address bytes = word32At bytes (fromIntegral (address - image4Base))
         in fmap (\bytes -> map (`wordAt` bytes) [0x623000, 0x623004, 0x623008, 0x62300c]) controlled
              == Right [carrierStart, carrierStart + 0x10, carrierStart, slotSetter]
              && isLeft (controlPatch contract 8))
    , check "the stage-3 class labels decode as owned classes"
        (let contract = either error id (decodeContract contractBytes)
             classOf property = armsClass <$> find ((== property) . armsProperty) (contractProperties contract)
         in map classOf [9, 10] == [Just OwnedReadOnlyField, Just OwnedWriteOnlyPair]
              && all (ownedClass . armsClass) (filter ((`elem` [9, 10]) . armsProperty) (contractProperties contract))
              && controlPatch contract 9 == Left "PROPERTY has no staged setter table word")
    , check "the next live arm and the retained image carry no guard"
        (refusal OwnedRole (guardHit (retiredGetter + 0x40)) == Just (UnarmedSite 6)
          && refusal RetainedRole retainedStep { stepHits = stepHits retainedStep ++ [hit (Guard RetiredGetterArm) retiredGetter [] stackPointer 0] } == Just (UnarmedSite 5))
    , check "a schema-2 owned image fills the hull but for the adapters, the carrier and the retained regions"
        (let contract = either error id (decodeContract hullContractBytes)
             wordAt address bytes = word32At bytes (fromIntegral (address - image4Base))
         in fmap (\bytes -> map (`wordAt` bytes) [getterEntry, setterEntry, retiredGetter, retiredSetter, 0x6245f8, getterCall, carrierStart])
              (roleImage4 (PlanInputs OwnedRole contract hullImages linked Nothing Nothing))
              == Right [getterAdapter, setterAdapter, fillWord, fillWord, islandWord, branchLink getterCall getterEntry, word32At linked 0])
    , check "a schema-2 plan guards every retired hull word at both spellings and no adapter, retained-region or carrier code word"
        (let contract = either error id (decodeContract hullContractBytes)
             sites = planSites <$> derivePlan (PlanInputs OwnedRole contract hullImages linked Nothing Nothing)
             at address = (>>= M.lookup address) . either (const Nothing) Just
         in at retiredGetter sites == Just (Guard RetiredGetterArm)
              && at (setterEntry + 4) sites == Just (Guard RetiredSetterArm)
              && at (setterEntry + 0x40000004) sites == Just (Guard RetiredSetterArm)
              && at (hullEnd' - 4) sites == Just (Guard RetiredSetterArm)
              && at (carrierEnd - 4) sites == Just (Guard CarrierFill)
              && at getterEntry sites == Just GetterEntry && at setterEntry sites == Just SetterEntry
              && all (\address -> at address sites == Nothing) [0x6245f8, 0x624690, carrierStart, carrierStart + 4, getterCall])
    , check "a schema-2 control detours the setter entry through the next, guarded word to the owned setter"
        (let contract = either error id (decodeContract hullContractBytes)
             control = setterEntryControl contract
             detour = setterEntry + 4
             wordAt address bytes = word32At bytes (fromIntegral (address - image4Base))
             branchTarget site word = site + 8 + 4 * (word .&. 0xffffff)
             controlled = control >>= \word -> roleImage4 (PlanInputs ControlRole contract hullImages linked (Just word) Nothing)
         in fmap controlTarget control == Right detour
              && fmap (\bytes -> map (`wordAt` bytes) [setterEntry, getterEntry]) controlled == Right [0xe1a00000, getterAdapter]
              && fmap (\bytes -> branchTarget detour (wordAt detour bytes)) controlled == Right (branchTarget setterEntry setterAdapter)
              && isLeft (controlPatch contract 5)
              && isLeft (roleImage4 (PlanInputs ControlRole contract hullImages linked (Just (ControlWord Nothing [(setterEntry, 0)] detour)) Nothing)))
    , check "a control manifest decodes to the control it was written for"
        (let control = ControlWord Nothing [(setterEntry, 0xe1a00000), (setterEntry + 4, 0xea001f85)] (setterEntry + 4)
             slot = ControlWord (Just 7) [(0x62300c, slotSetter)] slotSetter
             manifestRoundTrip word = decodeControlManifest (BL.toStrict (A.encode (controlManifestValue word "dat" "image4" "owned" "staged")))
         in manifestRoundTrip control == Right (control, "dat", "image4") && manifestRoundTrip slot == Right (slot, "dat", "image4"))

    , check "an owned settings write from linked code outside the accessors is refused"
        (refusal OwnedRole ownedStep { stepRows = stepRows ownedStep ++ [cpuWrite 0x96cc80 1 (Just 3) [0] rustBody], stepAfter = poke [(0x96cc80, 3)] (stepAfter ownedStep) }
          == Just (ForeignWriter crawlRoute rustBody))
    , check "a settings write from outside the bound images is admitted and left to the compare"
        (let bootRom = retainedStep { stepRows = [cpuWrite written 1 (Just 0xab) [0] 0x80000100] } in
          fmap (map effectWriter . resultWrites) (crawlResult RetainedRole bootRom) == Just [OtherWriter] && compared bootRom ownedStep == Right ())

    , check "an accessor store beyond its settings range is refused"
        (refusal OwnedRole (ownedWith [setterIn, commit, storeHit BankStore8 0x384 0xab, setterOut]) == Just (AccessorStoreBound 2)
          && refusal OwnedRole (ownedWith [setterIn, commit, storeHit BankStore16 0x383 0xab, setterOut]) == Just (AccessorStoreBound 2))
    , check "an accessor store must be the write row its instruction made"
        (refusal OwnedRole (ownedWith [setterIn, commit, storeHit BankStore8 0x10 0xac, setterOut]) == Just (AccessorStoreUnmatched (StepInterval 1))
          && refusal OwnedRole (ownedWith [setterIn, commit, setterOut]) == Just (AccessorStoreUnmatched (StepInterval 1)))

    , check "a probe receipt, or one with another pre-key idle, is refused"
        (let base = observation OwnedRole ownedStep
             edited key value = base { obsReceipt = case A.eitherDecodeStrict' (obsReceipt base) of
               Right (Object fields) -> BL.toStrict (A.encode (Object (KM.insert key value fields)))
               _ -> error "test receipt is not an object" }
             refusedWith key value = either Just (const Nothing) (qualify bindingsAdmitted OwnedRole (edited key value))
         in refusedWith "probe" (object ["path" .= ["right" :: T.Text]]) == Just (ReceiptShape "probe")
              && refusedWith "pre_key_idle_instructions" (A.toJSON (0 :: Int)) == Just (ReceiptShape "pre-key idle")
              && refusedWith "post_settle_prefix_ticks" (A.toJSON (0 :: Int)) == Just (ReceiptShape "post-settle prefix ticks"))
    , check "a step without its pre-key idle or bounded prefix endpoint is refused"
        (let base = observation OwnedRole ownedStep
             refusedWithout key =
               let withoutIdle = case A.eitherDecodeStrict' (obsSteps base) of
                     Right (Array steps) -> BL.toStrict (A.encode (Array (fmap (\step -> case step of
                       Object fields -> Object (KM.insert key Null fields)
                       other -> other) steps)))
                     _ -> error "test steps are not an array"
                   edited = base { obsSteps = withoutIdle }
                   rebound = edited { obsReceipt = case A.eitherDecodeStrict' (obsReceipt edited) of
                     Right (Object fields) -> BL.toStrict (A.encode (Object (KM.insert "files" (object
                       [ "spec.json" .= digestOf (obsSpec edited), "reset-observation.json" .= digestOf (obsReset edited)
                       , "hits.jsonl" .= digestOf (obsHits edited), "steps.json" .= digestOf withoutIdle
                       , "emulator-session.json" .= digestOf (obsSession edited) ]) fields)))
                     _ -> error "test receipt is not an object" }
                in case qualify bindingsAdmitted OwnedRole rebound of
                     Left (StepShape 1 _) -> True
                     _ -> False
          in all refusedWithout ["idle", "bounded_post_settle_prefix"])
    , check "a bounded prefix endpoint is admitted at the exact tick boundary"
        (refusal OwnedRole ownedStep == Nothing)
    , check "unaligned endpoint nodes leave identity words unavailable"
        (let ui first = object
               [ "menu" .= uiView 0x9571d0 [0x1001, 0, 0]
               , "depth" .= uiView 0x9575f4 [0], "node" .= uiView 0x1000 [first, 0, 5] ]
          in case (parseLocalUiMenu (ui 0x100), parseLocalUiMenu (ui 0x200)) of
               (Right left, Right right) -> left == right
               _ -> False)
    , check "matching pending source-mode transitions compare as a finite prefix"
        (let pending = prefixWith ["state", "source_mode"] (uiView 0x9500ec [4, 5, 3, 3, 5, 1, 0, 0, 1, 1])
          in compared retainedStep { stepPrefix = pending } ownedStep { stepPrefix = pending } == Right ())
    , check "early, late, advanced or malformed prefix evidence is refused"
        (all (\endpoint -> case refusal OwnedRole ownedStep { stepPrefix = endpoint } of
            Just (StepShape 1 _) -> True
            _ -> False)
          [ prefixWith ["elapsed", "ticks"] (A.toJSON (tickValue postSettle - 1))
          , prefixWith ["elapsed", "ticks"] (A.toJSON (tickValue postSettle + 1))
          , prefixWith ["state", "instructions"] (A.toJSON (11 :: Int))
          , prefixWith ["state", "source_mode"] (uiView 0x9500ec [0])
          , prefixWith ["state", "source_mode"] (object
              ["address" .= (0x9500ec :: Int), "physical" .= ("00000000" :: T.Text), "spellings" .= ([] :: [Value])]) ])
    , check "each compared source-mode field is part of the finite boundary"
        (all (\index ->
          let words' = [if offset == index then 1 else if offset == 0 then 3 else 0 | offset <- [0..9 :: Int]]
          in compared retainedStep ownedStep { stepPrefix = prefixWith ["state", "source_mode"] (uiView 0x9500ec words') }
              == Left (PressDiffers crawlRoute)) [0, 1, 2, 3, 5, 9])
    , check "uninterpreted source-mode bytes remain raw evidence without changing finite-prefix equality"
        (all (\index ->
          let bytes=flippedByte index (littleWords [3,0,0,0,0,0,0,0,0,0])
          in compared retainedStep ownedStep {stepPrefix=prefixWith ["state","source_mode"] (uiViewBytes 0x9500ec bytes)} == Right ())
          ([16..19] ++ [24..35] ++ [37..39]))
    , check "a crawl route whose last step lacks its recorded effects is refused"
        (refusal OwnedRole (ownedWith (filter (/= commit) (stepHits ownedStep))) == Just (RouteEffectsDiffer crawlRoute))
    , check "recorded and observed effects that differ only in a time-driven block's repeat count qualify, with the observed count"
        (effectBlocksWith ([SetterEffect 5, CommitEffect] ++ map SetterEffect timeDrivenSequence) (blockHits ++ blockHits ++ blockHits) == Right (Just [(2, 2, 3)]))
    , check "recorded and observed effects that differ outside the block are refused"
        (effectBlocksWith ([SetterEffect 5, CommitEffect] ++ map SetterEffect timeDrivenSequence) (blockHits ++ setterPair 6 ++ blockHits)
           == Left (RouteEffectsDiffer crawlRoute)
          && effectBlocksWith ([SetterEffect 5, CommitEffect] ++ map SetterEffect timeDrivenSequence) (setterPair 6 ++ blockHits)
               == Left (RouteEffectsDiffer crawlRoute))

    , check "a getter key's results compare over the whole observation as sequences with repeats collapsed"
        (compared retainedStep (ownedWith (stepHits ownedStep ++ [getterIn, getterOut])) == Right ()
          && compared retainedStep (ownedWith (take 5 (stepHits ownedStep) ++ [hit GetterReturn (getterCall + 4) [0xac] stackPointer 0])) == Left (GetterSequenceDiffers getterKey5 0))
    , check "a getter result returned out of order is a difference even when the result set is equal"
        (compared (retainedStep { stepHits = stepHits retainedStep ++ getterPair 5 0xac })
                  (ownedWith (stepHits ownedStep ++ getterPair 5 0xac ++ getterPair 5 0xab)) == Left (GetterSequenceDiffers getterKey5 2)
          && compared (retainedStep { stepHits = stepHits retainedStep ++ getterPair 5 0xac })
                      (ownedWith (stepHits ownedStep ++ getterPair 5 0xac ++ getterPair 5 0xac)) == Right ())
    , check "a getter key's calls that slip across a step boundary compare equal"
        (comparedChains [retainedStep { stepHits = stepHits retainedStep ++ getterPair 5 0xac }, childWith []]
                        [ownedStep, childWith (getterPair 5 0xac)] == Right ())
    , check "a stale value inside a run of a getter key's results is refused"
        (compared (retainedStep { stepHits = stepHits retainedStep ++ getterPair 5 0xac ++ getterPair 5 0xac ++ getterPair 5 0xac })
                  (ownedWith (stepHits ownedStep ++ getterPair 5 0xac ++ getterPair 5 0xab ++ getterPair 5 0xac)) == Left (GetterSequenceDiffers getterKey5 2))
    , check "a getter key present in only one role is refused"
        (compared retainedStep (ownedWith (stepHits ownedStep ++ getterPair 6 0xcd)) == Left (GetterSequenceDiffers getterKey6 0))
    , check "getter calls under different keys interleave freely"
        (compared (retainedStep { stepHits = stepHits retainedStep ++ getterPair 6 0xcd ++ getterPair 5 0xac })
                  (ownedWith (stepHits ownedStep ++ getterPair 5 0xac ++ getterPair 6 0xcd)) == Right ())
    , check "the stale getter control sets a call inside the first run to the next distinct value and is refused"
        (let owned = observation OwnedRole (ownedWith (stepHits ownedStep ++ getterPair 5 0xab ++ getterPair 5 0xab ++ getterPair 5 0xac))
             retained = observation RetainedRole retainedStep { stepHits = stepHits retainedStep ++ getterPair 5 0xab ++ getterPair 5 0xab ++ getterPair 5 0xac }
         in case runControl bindingsAdmitted OwnedRole owned retained StaleGetterResult of
              Right result -> controlStale result == Just (StaleGetter getterKey5 1 7 0xab 0xac)
                && controlObserved result == Left (GetterSequenceDiffers getterKey5 2)
              Left _ -> False)
    , check "the stale getter control sets a call inside a later run to the previous distinct value and is refused"
        (let owned = observation OwnedRole (ownedWith (stepHits ownedStep ++ getterPair 5 0xac ++ getterPair 5 0xac ++ getterPair 5 0xac))
             retained = observation RetainedRole retainedStep { stepHits = stepHits retainedStep ++ getterPair 5 0xac ++ getterPair 5 0xac ++ getterPair 5 0xac }
         in case runControl bindingsAdmitted OwnedRole owned retained StaleGetterResult of
              Right result -> controlStale result == Just (StaleGetter getterKey5 2 9 0xac 0xab)
                && controlObserved result == Left (GetterSequenceDiffers getterKey5 2)
              Left _ -> False)
    , check "a write that leaves its preimage is not compared"
        (compared retainedStep ownedStep { stepRows = stepRows ownedStep ++ [cpuWrite 0x96cc80 1 (Just 0) [0] retiredSetter] } == Right ())
    , check "settle status differs only through an exhausted budget; the menu always counts"
        (compared retainedStep ownedStep { stepStatus = "budget-exhausted" } == Right ()
          && compared retainedStep ownedStep { stepMenu = object ["node" .= (2 :: Int)] } == Left (PressDiffers crawlRoute)
          && compared retainedStep ownedStep { stepPrefix = prefixWith ["state", "ui", "menu"] (uiView 0x9571d0 [0x1000, 0, 1]) } == Left (PressDiffers crawlRoute))
    , check "a child step continues from its parent step's after regions"
        (let child = retainedStep { stepHits = [], stepRows = [] } in
          either Just (const Nothing) (qualify bindingsAdmitted RetainedRole (chain RetainedRole [retainedStep, child])) == Nothing
            && either Just (const Nothing) (qualify bindingsAdmitted RetainedRole (chain RetainedRole [retainedStep, child { stepBefore = Just zeroRegions }]))
                 == Just (RegionContinuity (StepInterval 2)))

    , check "setter sequences that differ only in a time-driven block's repeat count compare equal and report the counts"
        (fmap blockRunsOf (comparedSetters (otherSetter : concat (replicate 3 timeBlock) ++ [otherSetter]) (otherSetter : timeBlock ++ [otherSetter]))
          == Right (Just ([(1, 1, 3)], [(1, 1, 1)])))
    , check "a block with a different value is not collapsed into the run before it"
        (fmap blockRunsOf (comparedSetters (timeBlock ++ changedBlock) (timeBlock ++ changedBlock)) == Right (Just ([(0, 0, 1), (1, 4, 1)], [(0, 0, 1), (1, 4, 1)]))
          && comparedSetters (timeBlock ++ changedBlock) (timeBlock ++ timeBlock) `refusedAs` SetterSequenceDiffers crawlRoute
          && comparedSetters (timeBlock ++ changedBlock) timeBlock `refusedAs` SetterSequenceDiffers crawlRoute)
    , check "a repeat with another result or another caller is not collapsed into the run before it"
        (comparedSetters (timeBlock ++ map (\c -> c { callResult = Just 0 }) timeBlock) (timeBlock ++ timeBlock) `refusedAs` SetterSequenceDiffers crawlRoute
          && comparedSetters (timeBlock ++ map (\c -> c { callReturn = 0x630a08 }) timeBlock) (timeBlock ++ timeBlock) `refusedAs` SetterSequenceDiffers crawlRoute)
    , check "a call between two blocks keeps them as separate runs"
        (fmap blockRunsOf (comparedSetters (timeBlock ++ [otherSetter] ++ timeBlock) (timeBlock ++ [otherSetter] ++ timeBlock))
          == Right (Just ([(0, 0, 1), (2, 5, 1)], [(0, 0, 1), (2, 5, 1)]))
          && comparedSetters (timeBlock ++ [otherSetter] ++ timeBlock) (timeBlock ++ timeBlock ++ [otherSetter]) `refusedAs` SetterSequenceDiffers crawlRoute)
    , check "the ordered refresh suffix cannot be omitted, reordered, or assigned another value"
        (let expected = [otherSetter] ++ timeBlock
             reordered = [otherSetter] ++ reverse timeBlock
             wrongValue = [otherSetter] ++ changedBlock
         in not (isLeft (comparedSetters expected expected))
              && comparedSetters expected [otherSetter] `refusedAs` SetterSequenceDiffers crawlRoute
              && comparedSetters expected reordered `refusedAs` SetterSequenceDiffers crawlRoute
              && comparedSetters expected wrongValue `refusedAs` SetterSequenceDiffers crawlRoute)
    ]
  matrixResults <- mapM (uncurry check) matrixParserCases
  bankResults <- mapM (uncurry check) bankCallerCases
  bankProofResults <- mapM (uncurry check) bankProofBindingCases
  sourceModeResults <- mapM (uncurry check) sourceModeCases
  let allResults = results ++ matrixResults ++ bankResults ++ bankProofResults ++ sourceModeResults
  unless (and allResults) exitFailure
  putStrLn ("property-pair-tests: " ++ show (length allResults) ++ " passed")
