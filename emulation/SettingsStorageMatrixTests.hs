{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module SettingsStorageMatrixTests (matrixConsumerCases) where

import Data.Aeson (Value(..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word16, Word32, Word64)
import Emulation.PropertyPairCheck
import Emulation.PropertyPairPlan hiding (image4Base)
import Emulation.PropertyPairRoutes (AttachPhase (..))
import qualified Emulation.SettingsStorageMatrix as Storage
import Emulation.Sha256 (Sha256, sha256, sha256Hex)
import EmulatorUsb (CrawlKey(..), ModeDial(..), crawlKeyWire, modeDialName)
import PropertyPairFixture hiding (bindings, compared, observation, refusal)
import SettingsStorageEeFixture
import SettingsStorageFixtureWire
import SettingsStorageS1Fixture

newtype RecordImage = RecordImage B.ByteString deriving Eq
newtype ControllerState = ControllerState Word16 deriving Eq
newtype ControllerEvent = ControllerEvent Word16 deriving Eq
newtype InstructionCounter = InstructionCounter Word64 deriving (Eq, Ord)

data FixtureState = FixtureState
  { activeImage :: BankImage
  , alternateImage :: BankImage
  , previousImage :: BankImage
  , backingImage :: BankImage
  , previousRecord :: RecordImage
  , workingRecord :: RecordImage
  , selectedProfile :: Storage.ProfileSlot
  , controller30State :: ControllerState
  , controller30Event :: ControllerEvent
  , controller74State :: ControllerState
  } deriving Eq

data FixtureStep
  = SaveChanged | SaveUnchanged | ChangeLivePayload | SelectCustom | SelectLive
  | SelectCustomAgain | SelectLiveAgain
  | CaptureAlternate | EditAlternate | RestoreAlternate | HalfPressApplication
  | RepeatRoundTrips | RebootCommitted
  deriving (Eq, Ord, Show, Enum, Bounded)

bankBytes :: BankImage -> B.ByteString
bankBytes (BankImage bytes) = bytes

recordData :: RecordImage -> B.ByteString
recordData (RecordImage bytes) = bytes

replaceBytes :: Int -> B.ByteString -> B.ByteString -> B.ByteString
replaceBytes offset bytes original = B.take offset original <> bytes <> B.drop (offset+B.length bytes) original

activeA, activeB, backingA, backingB, restoredBacking :: BankImage
activeA = BankImage (B.replicate 0x1c2 0x11)
activeB = BankImage (B.replicate 0x1c2 0x22)
backingA = BankImage (B.replicate 410 0x11 <> B.replicate 40 0)
backingB = BankImage (replaceBytes 0x140 (B.replicate 40 0x22) (replaceBytes 0 (B.replicate 0x118 0x22) (bankBytes backingA)))
restoredBacking = BankImage (replaceBytes 0 (B.take 0x118 (B.drop 16 (bankBytes activeA))) (bankBytes backingB))

blankRecord, appendCandidate, sealedRecord, savedWorking :: RecordImage
blankRecord = RecordImage (B.replicate 52 0)
appendCandidate = RecordImage (littleWords [0] <> B.replicate 40 0x11 <> littleWords [0,0])
sealedRecord = RecordImage (littleWords [8] <> B.replicate 40 0x11 <> littleWords [1,8])
savedWorking = RecordImage (littleWords [8] <> B.replicate 40 0x11 <> littleWords [0,0])

initialState, savedState, candidateState, committedState, editedLiveState, customState, liveState, capturedState, editedState, restoredState, appliedState, rebootedState :: FixtureState
initialState = FixtureState activeA (BankImage (B.replicate 0x1c2 0)) (BankImage (B.replicate 0x1c2 0)) backingA blankRecord blankRecord Storage.LiveProfile (ControllerState 0) (ControllerEvent 0) (ControllerState 0)
candidateState = initialState {workingRecord=appendCandidate}
committedState = candidateState {previousRecord=sealedRecord}
savedState = committedState {workingRecord=savedWorking}
editedLiveState = savedState
  { activeImage=BankImage (replaceBytes 0x140 (B.replicate 40 0x22) (bankBytes activeA))
  , backingImage=BankImage (replaceBytes 0x140 (B.replicate 40 0x22) (bankBytes backingA))
  }
customState = savedState {selectedProfile=Storage.StoredCustom}
liveState = customState {selectedProfile=Storage.LiveProfile}
capturedState = liveState {alternateImage=activeA}
editedState = capturedState {activeImage=activeB,backingImage=backingB}
restoredState = editedState {activeImage=activeA,backingImage=restoredBacking}
appliedState = restoredState {controller30State=ControllerState 3,controller74State=ControllerState 3}
rebootedState = initialState {previousRecord=sealedRecord,workingRecord=sealedRecord}

stateBounds :: FixtureStep -> (FixtureState,FixtureState)
stateBounds step = case step of
  SaveChanged -> (initialState,savedState)
  SaveUnchanged -> (savedState,savedState)
  ChangeLivePayload -> (savedState,editedLiveState)
  SelectCustom -> (editedLiveState,customState)
  SelectLive -> (customState,liveState)
  SelectCustomAgain -> (liveState,customState)
  SelectLiveAgain -> (customState,liveState)
  CaptureAlternate -> (liveState,capturedState)
  EditAlternate -> (capturedState,editedState)
  RestoreAlternate -> (editedState,restoredState)
  HalfPressApplication -> (restoredState,appliedState)
  RepeatRoundTrips -> (appliedState,appliedState)
  RebootCommitted -> (appliedState,rebootedState)

stateSegments :: FixtureState -> [(Word32,B.ByteString)]
stateSegments state =
  [ (0x96cc64,bankBytes (activeImage state)),(0x96ce26,bankBytes (alternateImage state))
  , (0x89ec00,bankBytes (previousImage state)),(0x9c281c,bankBytes (backingImage state))
  , (0x89f838,recordData (previousRecord state)),(0x89f874,recordData (workingRecord state))
  ]

stateRegions :: FixtureState -> Regions
stateRegions state = foldl' place zeroRegions (stateSegments state)
  where
    place regions (address,bytes) = case M.lookupLE address regions of
      Just (base,original) | toInteger address+toInteger (B.length bytes) <= toInteger base+toInteger (B.length original) ->
        M.insert base (replaceBytes (fromIntegral (address-base)) bytes original) regions
      _ -> error "fixture state leaves watched regions"

stateWire :: FixtureState -> Value
stateWire state = object
  [ "bank0" .= wireBytes (bankBytes (activeImage state)),"bank1" .= wireBytes (bankBytes (alternateImage state))
  , "previous" .= wireBytes (bankBytes (previousImage state)),"backing" .= wireBytes (bankBytes (backingImage state))
  , "custom_previous" .= wireBytes (recordData (previousRecord state)),"custom_working" .= wireBytes (recordData (workingRecord state))
  , "mode_class" .= (if selectedProfile state == Storage.StoredCustom then 3 else 0 :: Word32)
  , "c_scanner_row" .= wireBytes (littleWords [0x317,0,0x101,3,0x37243726,8,0])
  , "c_input_bitmap" .= (if selectedProfile state == Storage.StoredCustom then 0x02000000 else 0 :: Word32)
  , "controller30_state" .= (case controller30State state of ControllerState value -> value)
  , "controller30_event" .= (case controller30Event state of ControllerEvent value -> value)
  , "controller74_state" .= (case controller74State state of ControllerState value -> value)
  ]

matrixImages :: M.Map ImageName B.ByteString
matrixImages = M.insert Image4 canonical (M.insert Image5 five images)
  where
    canonical = installEeConversionTable $ foldl' (\bytes (address,word) -> putWord address word bytes)
      (composedCanonical <> B.replicate (0x210000-B.length composedCanonical) 0)
      ([pin | pin@(address,_) <- Storage.lifecycleWords,address < 0xbf1a00]
       ++ zip [0x8284bc,0x8284c0..] [0x317,0,0x101,3,0x37243726,8,0]
       ++ [(0x624644,branchLink 0x624644 getterEntry),(0x61c924,branchLink 0x61c924 0x6309c4)
          ,(0x61d010,branchLink 0x61d010 0x6a86c4)])
    five = foldl' (\bytes (address,word) -> let offset=fromIntegral (address-0xbf1a00)
      in replaceBytes offset (littleWords [word]) bytes) (B.replicate 0x200000 0)
      [pin | pin@(address,_) <- Storage.lifecycleWords,address >= 0xbf1a00]

matrixImage4 :: B.ByteString
matrixImage4 = installEeConversionTable $ foldl' (\bytes (address,word) -> putWord address word bytes)
  (composedImage <> B.replicate (0x210000-B.length composedImage) 0)
  ([pin | pin@(address,_) <- Storage.lifecycleWords,address < 0xbf1a00]
   ++ zip [0x8284bc,0x8284c0..] [0x317,0,0x101,3,0x37243726,8,0]
   ++ [(0x624644,branchLink 0x624644 getterEntry),(0x61c924,branchLink 0x61c924 0x6309c4)
      ,(0x61d010,branchLink 0x61d010 0x6a86c4)])

installEeConversionTable :: B.ByteString -> B.ByteString
installEeConversionTable bytes = putWord 0x81b454 0x775ca4
  (replaceBytes (fromIntegral (0x775ca4-image4Base)) eeConversionTable bytes)

encodeWire :: Value -> B.ByteString
encodeWire = BL.toStrict . A.encode

decodeWire :: B.ByteString -> Value
decodeWire = either error id . A.eitherDecodeStrict'

matrixImageHashes :: M.Map ImageName Sha256
matrixImageHashes = M.map sha256 matrixImages

matrixImage4Hash :: Sha256
matrixImage4Hash = sha256 matrixImage4

matrixContract :: B.ByteString
matrixContract = encodeWire (foldl' (\value (path,replacement) -> wireField path replacement value)
  (decodeWire composedContractBytes)
  [ (["linked_sha256"],A.toJSON (sha256Hex matrixImage4Hash)),(["linked_bytes"],A.toJSON (B.length matrixImage4))
  , (["staged_image4_sha256"],A.toJSON (sha256Hex matrixImage4Hash))
  , (["property_contract","scan","images"],A.toJSON
      [object ["image" .= imageNameWire name,"base" .= base,"bytes" .= B.length (matrixImages M.! name),"sha256" .= sha256Hex (matrixImageHashes M.! name)]
      | (name,base) <- (Image4,image4Base):otherImages])
  ])

stepName :: FixtureStep -> T.Text
stepName step = case step of
  SaveChanged -> "save-changed"
  SaveUnchanged -> "save-unchanged"
  ChangeLivePayload -> "change-live-payload"
  SelectCustom -> "select-custom"
  SelectLive -> "select-live"
  SelectCustomAgain -> "select-custom-again"
  SelectLiveAgain -> "select-live-again"
  CaptureAlternate -> "capture-alternate"
  EditAlternate -> "edit-alternate"
  RestoreAlternate -> "restore-alternate"
  HalfPressApplication -> "half-press-application"
  RepeatRoundTrips -> "repeat-setting-round-trips"
  RebootCommitted -> "reboot-committed"

stepAction :: FixtureStep -> Storage.StorageAction
stepAction step = case step of
  SelectCustom -> Storage.SetModeDial ModeDialCustom
  SelectLive -> Storage.SetModeDial ModeDialManual
  SelectCustomAgain -> Storage.SetModeDial ModeDialCustom
  SelectLiveAgain -> Storage.SetModeDial ModeDialManual
  ChangeLivePayload -> Storage.PressKey KeyRight
  EditAlternate -> Storage.PressKey KeyRight
  RestoreAlternate -> Storage.PressKey KeyBack
  HalfPressApplication -> Storage.PressHalfShutter
  RebootCommitted -> Storage.ColdReboot
  SaveChanged -> Storage.PressKey KeyOk
  SaveUnchanged -> Storage.PressKey KeyOk
  CaptureAlternate -> Storage.PressKey KeyOk
  RepeatRoundTrips -> Storage.PressKey KeyOk

stepChecks :: Storage.StorageSetting -> FixtureStep -> [Value]
stepChecks family step = case step of
  SaveChanged -> map A.toJSON [Storage.CustomCommit Storage.Changed,Storage.CurrentSlot Storage.LiveProfile]
  SaveUnchanged -> [A.toJSON (Storage.CustomCommit Storage.Unchanged)]
  ChangeLivePayload -> [payload "different" "backing"]
  SelectCustom -> [A.toJSON (Storage.CurrentSlot Storage.StoredCustom),payload "same" "active-bank",payload "same" "backing"]
  SelectLive -> [A.toJSON (Storage.CurrentSlot Storage.LiveProfile)]
  SelectCustomAgain -> [A.toJSON (Storage.CurrentSlot Storage.StoredCustom),payload "same" "active-bank",payload "same" "backing"]
  SelectLiveAgain -> [A.toJSON (Storage.CurrentSlot Storage.LiveProfile)]
  CaptureAlternate -> [A.toJSON (Storage.BankCopy Storage.AlternateImage Storage.Capture)]
  EditAlternate -> []
  RestoreAlternate -> [A.toJSON (Storage.BankCopy Storage.AlternateImage Storage.Restore)]
  HalfPressApplication -> [A.toJSON Storage.UiHandback]
  RepeatRoundTrips -> [A.toJSON (Storage.RepeatSetting family)]
  RebootCommitted -> [object ["kind" .= ("cold-boot-record"::T.Text),"reference" .= stepName SaveChanged]]
  where
    payload relation target = object
      [ "kind" .= ("custom-payload"::T.Text),"relation" .= (relation::T.Text),"target" .= (target::T.Text)
      , "reference" .= object ["step" .= stepName SaveChanged,"target" .= ("working-record"::T.Text)]
      ]

matrixWorkload :: B.ByteString
matrixWorkload = matrixWorkloadFor Storage.FilmSimulation

matrixWorkloadForSetting :: Storage.StorageSetting -> B.ByteString
matrixWorkloadForSetting family = encodeWire (object
  [ "schema" .= (1::Int),"kind" .= ("settings-storage-lifecycle"::T.Text)
  , "source" .= object
      [ "decomp_commit" .= ("c44204c8c1c87e00ed15a7d33572968afadd1b29"::T.Text)
      , "catalog_commit" .= ("1a92977f50f5bed7cd77a9bb1cb23271fdb0800c"::T.Text)
      , "controls_commit" .= ("1a92977f50f5bed7cd77a9bb1cb23271fdb0800c"::T.Text)
      , "image4_sha256" .= sha256Hex (matrixImageHashes M.! Image4),"image5_sha256" .= sha256Hex (matrixImageHashes M.! Image5)
      ]
  , "steps" .= [object ["name" .= stepName step,"action" .= stepAction step,"checks" .= stepChecks family step] | step <- [minBound..maxBound]]
  ])

matrixFamilies :: [Storage.StorageSetting]
matrixFamilies = [Storage.FilmSimulation, Storage.WhiteBalanceShift]

matrixWorkloads :: M.Map Storage.StorageSetting B.ByteString
matrixWorkloads = M.fromList [(family, matrixWorkloadForSetting family) | family <- matrixFamilies]

matrixWorkloadFor :: Storage.StorageSetting -> B.ByteString
matrixWorkloadFor family = matrixWorkloads M.! family

matrixWorkloadDigests :: M.Map Storage.StorageSetting Sha256
matrixWorkloadDigests = M.map sha256 matrixWorkloads

matrixDefinitions :: M.Map Storage.StorageSetting Storage.StorageMatrix
matrixDefinitions = M.fromList
  [(family, either error id (Storage.decodeStorageMatrix (matrixWorkloadFor family))) | family <- matrixFamilies]

matrixDefinition :: Storage.StorageMatrix
matrixDefinition = matrixDefinitions M.! Storage.FilmSimulation

matrixBindings :: Bindings
matrixBindings = matrixBindingsForSetting Storage.FilmSimulation

matrixBindingsForSetting :: Storage.StorageSetting -> Bindings
matrixBindingsForSetting family = composedBindings
  {bindRoutes=matrixWorkloadFor family,bindContract=matrixContract,bindLinked=matrixImage4,bindImages=matrixImages}

matrixAdmittedContexts :: M.Map Storage.StorageSetting AdmittedBindings
matrixAdmittedContexts = M.fromList
  [(family, either (error . show) id (admitBindings (matrixBindingsForSetting family))) | family <- matrixFamilies]

matrixAdmittedFor :: Storage.StorageSetting -> AdmittedBindings
matrixAdmittedFor family = matrixAdmittedContexts M.! family

matrixAdmitted :: AdmittedBindings
matrixAdmitted = matrixAdmittedFor Storage.FilmSimulation

data WriteWitness = WriteWitness { writeTrace :: [Value], writeRows :: [Value] }

changedWrites :: Role -> Regions -> Regions -> WriteWitness
changedWrites role before after = WriteWitness traces rows
  where
    changes =
      [ (start+fromIntegral index,old,new)
      | (start,oldBytes) <- M.toAscList before
      , let newBytes=case M.lookup start after of Just bytes -> bytes; Nothing -> error "missing after region"
      , (index,(old,new)) <- zip [0::Int ..] (zip (B.unpack oldBytes) (B.unpack newBytes)),old /= new
      ]
    accessor address = if address >= 0x89f834 && address < 0x89f8a8 then CustomByte else StorageByte
    site wanted = case [address+fromIntegral (index*4) | (kind,address,_,index) <- composedBoundaries,kind == wanted] of
      address:_ -> address
      [] -> error "missing fixture write boundary"
    destination address = if address >= 0x9c281c && address < 0x9c281c+1144 then address+0x40000000 else address
    traces = if role == RetainedRole then [] else
      [matrixHit role (AccessorStore (accessor address)) (site (accessor address)) [destination address,fromIntegral new] 0 (matrixStack role) 0 [] | (address,_,new) <- changes]
    rows =
      [cpuWrite address 1 (Just (toInteger new)) [old] (if role == RetainedRole then 0x61daa8 else site (accessor address))
      | (address,old,new) <- changes]

stateWrites :: Role -> FixtureState -> FixtureState -> WriteWitness
stateWrites role before after = changedWrites role (stateRegions before) (stateRegions after)

copySnapshots :: FixtureState -> [B.ByteString]
copySnapshots state = map bankBytes [activeImage state,alternateImage state,previousImage state,backingImage state]

refreshSnapshots :: FixtureState -> [B.ByteString]
refreshSnapshots state =
  [recordData (previousRecord state),recordData (workingRecord state),B.take 40 (B.drop 0x140 (bankBytes (backingImage state)))]

refreshHit :: Role -> Storage.LifecycleSite -> FixtureState -> Value
refreshHit role site state = matrixHit role (StorageSite site) address arguments 0 (matrixStack role) lr (refreshSnapshots state)
  where
    address = case site of
      Storage.CustomRefreshEntry -> if role == RetainedRole then 0x6a86c4 else customRefreshBody
      Storage.CustomRefreshReturn -> 0x61d014
      _ -> error "not a refresh fixture boundary"
    arguments = case site of Storage.CustomRefreshReturn -> [0]; _ -> []
    lr = case site of Storage.CustomRefreshEntry -> 0x61d014; _ -> 0

appendHit :: Role -> Value
appendHit role = matrixHit role Area3Append address arguments 0 (matrixStack role) 0
  [recordData appendCandidate,recordData blankRecord,B.replicate 40 0x11,littleWords [0x89f8f8]]
  where
    address = if role == RetainedRole then area3Append else composedCodeStart+0x1e0
    arguments = if role == RetainedRole then [0x89f8f8,0,0x89f874] else [0x1000,0x2000,0x89f874]

programHit :: Role -> Value
programHit role = matrixHit role RecordProgram 0x6ab2f4 [0x89fa80,0,0,0x600] 0 (matrixStack role) 0
  [programBytes]

readHit :: Role -> Value
readHit role = matrixHit role RecordRead 0x6aad94 [0xb00000,0x8a0080] 0 (matrixStack role) 0 []

profileHits :: Role -> Storage.ProfileSlot -> [Value]
profileHits role slot =
  [matrixHit role GetterEntry getterEntry [7,0,0,selector] 0 (matrixStack role) 0x624648 []
  ,matrixHit role GetterReturn 0x624648 [0] 0 (matrixStack role) 0 []]
  where selector=case slot of Storage.StoredCustom -> 0; Storage.LiveProfile -> 1

repeatHits :: Role -> Storage.StorageSetting -> [Value]
repeatHits role family = concat (replicate 2
  [matrixHit role SetterEntry setterEntry [property,3,0,1] 0 (matrixStack role) (setterCall+4) []
  ,matrixHit role SetterReturn (setterCall+4) [1] 0 (matrixStack role) 0 []
  ,matrixHit role GetterEntry getterEntry [property,0,0,1] 0 (matrixStack role) (getterCall+4) []
  ,matrixHit role GetterReturn (getterCall+4) [3] 0 (matrixStack role) 0 []])
  where property=case family of Storage.FilmSimulation -> 0xfe; Storage.WhiteBalanceShift -> 0xee

stepWitness :: Storage.StorageSetting -> Role -> FixtureStep -> WriteWitness
stepWitness family role step = case step of
  SaveChanged ->
    let first=stateWrites role initialState candidateState
        second=stateWrites role candidateState committedState
        third=stateWrites role committedState savedState
    in WriteWitness
      ([refreshHit role Storage.CustomRefreshEntry initialState] ++ writeTrace first
       ++ [appendHit role,programHit role,matrixHit role RecordProgramResult 0x6ab390 [0] 0 (matrixStack role) 0 []]
       ++ writeTrace second ++ writeTrace third ++ [refreshHit role Storage.CustomRefreshReturn savedState]
       ++ profileHits role Storage.LiveProfile)
      (writeRows first ++ writeRows second ++ writeRows third)
  SaveUnchanged -> WriteWitness [refreshHit role Storage.CustomRefreshEntry savedState,refreshHit role Storage.CustomRefreshReturn savedState] []
  CaptureAlternate -> copy Storage.Capture liveState capturedState
  RestoreAlternate -> copy Storage.Restore editedState restoredState
  SelectCustom -> withState (profileHits role Storage.StoredCustom)
  SelectLive -> withState (profileHits role Storage.LiveProfile)
  SelectCustomAgain -> withState (profileHits role Storage.StoredCustom)
  SelectLiveAgain -> withState (profileHits role Storage.LiveProfile)
  HalfPressApplication -> withState (s1ApplicationHits role)
  RepeatRoundTrips -> withState (repeatHits role family)
  RebootCommitted -> WriteWitness [] []
  ChangeLivePayload -> withState []
  EditAlternate -> withState []
  where
    withState extra = let (before,after)=stateBounds step; writes=stateWrites role before after
      in WriteWitness (writeTrace writes ++ extra) (writeRows writes)
    copy direction before after =
      let writes=stateWrites role before after
          entry=matrixHit role (StorageSite Storage.ExchangeAlternateEntry) 0x61daa8 [case direction of Storage.Capture -> 0; Storage.Restore -> 1] 0 (matrixStack role) 0x61c918 (copySnapshots before)
          returned=matrixHit role (StorageSite Storage.ExchangeAlternateReturn) 0x61c918 [0] 0 (matrixStack role) 0 (copySnapshots after)
      in WriteWitness ([entry] ++ writeTrace writes ++ [returned]) (writeRows writes)

newtype HitCursor = HitCursor Int deriving (Eq, Ord)
newtype StepNumber = StepNumber Int

data PreparedStep = PreparedStep
  { preparedKind :: FixtureStep
  , preparedNumber :: StepNumber
  , preparedFrom :: HitCursor
  , preparedTo :: HitCursor
  , preparedIdle :: InstructionCounter
  , preparedEnd :: InstructionCounter
  , preparedTrace :: [Value]
  , preparedRows :: [Value]
  }

counterValue :: InstructionCounter -> Word64
counterValue (InstructionCounter value) = value

cursorValue :: HitCursor -> Int
cursorValue (HitCursor value) = value

stamp :: InstructionCounter -> [Value] -> [Value]
stamp (InstructionCounter start) = zipWith (\index -> wireField ["instructions"] (A.toJSON (start+100*index))) [1::Word64 ..]

prepareSteps :: M.Map FixtureStep WriteWitness -> M.Map FixtureStep TraceMutation -> [Value] -> [PreparedStep]
prepareSteps witnesses changes bootTrace = build (StepNumber 1) (HitCursor (length bootTrace))
  (InstructionCounter (100*fromIntegral (length bootTrace))) [minBound..RepeatRoundTrips]
  where
    build _ _ _ [] = []
    build number@(StepNumber ordinal) from@(HitCursor first) previous (kind:rest) =
      let witness=case M.lookup kind witnesses of Just value -> value; Nothing -> error "matrix step has no shared witness"
          rawTrace=case M.lookup kind changes of Just change -> change (writeTrace witness); Nothing -> writeTrace witness
          idle=InstructionCounter (counterValue previous+case stepAction kind of Storage.SetModeDial _ -> 0; _ -> instructionsValue preKeyIdle)
          spanCount=100*fromIntegral (max (length rawTrace) (length (writeRows witness)))+1000
          end=InstructionCounter (counterValue idle+spanCount+10)
          to=HitCursor (first+length rawTrace)
          prepared=PreparedStep kind number from to idle end (stamp idle rawTrace) (stamp idle (writeRows witness))
      in prepared:build (StepNumber (ordinal+1)) to end rest

matrixWitnesses :: M.Map (Storage.StorageSetting, Role) (M.Map FixtureStep WriteWitness)
matrixWitnesses = M.fromList
  [ ((family, role), M.fromList [(step, stepWitness family role step) | step <- [minBound .. RepeatRoundTrips]])
  | family <- matrixFamilies, role <- [RetainedRole, OwnedRole] ]

matrixWitnessesFor :: Storage.StorageSetting -> Role -> M.Map FixtureStep WriteWitness
matrixWitnessesFor family role = matrixWitnesses M.! (family, role)

matrixSpecFor :: Storage.StorageSetting -> Role -> B.ByteString
matrixSpecFor family role = either (error . show) id (specFor (matrixAdmittedFor family) role)

matrixSpecDigests :: M.Map (Storage.StorageSetting, Role) Sha256
matrixSpecDigests = M.fromList
  [ ((family, role), sha256 (matrixSpecFor family role))
  | family <- matrixFamilies, role <- [RetainedRole, OwnedRole] ]

matrixFreshLoads :: M.Map Role WriteWitness
matrixFreshLoads = M.fromList [(role, freshLoad role) | role <- [RetainedRole, OwnedRole]]

matrixChildResets :: M.Map Role B.ByteString
matrixChildResets = M.fromList [(role, freshReset (matrixFreshLoads M.! role)) | role <- [RetainedRole, OwnedRole]]

matrixChildHits :: M.Map Role [Value]
matrixChildHits = M.fromList
  [(role, stamp (InstructionCounter 100) (writeTrace (matrixFreshLoads M.! role))) | role <- [RetainedRole, OwnedRole]]

matrixInitialReset :: B.ByteString
matrixInitialReset = initialReset []

matrixInitialResetDigest :: Sha256
matrixInitialResetDigest = sha256 matrixInitialReset

matrixAnalysisValue :: Value
matrixAnalysisValue = object [K.fromText (imageNameWire name) .= sha256Hex digest | (name, digest) <- M.toList matrixImageHashes]

matrixRoleImageDigests :: M.Map Role Sha256
matrixRoleImageDigests = M.fromList
  [(RetainedRole, matrixImageHashes M.! Image4), (OwnedRole, matrixImage4Hash)]

matrixContractDigest, matrixSymbolsDigest, matrixElfDigest :: Sha256
matrixContractDigest = sha256 matrixContract
matrixSymbolsDigest = sha256 (bindSymbols matrixBindings)
matrixElfDigest = case bindElf matrixBindings of
  Just bytes -> sha256 bytes
  Nothing -> error "matrix ELF is absent"

matrixRunDigest, matrixRomDigest, matrixNandDigest, matrixBridgeDigest :: Sha256
matrixRunDigest = sha256 "run"
matrixRomDigest = sha256 "rom"
matrixNandDigest = sha256 "nand"
matrixBridgeDigest = sha256 "bridge"

prefixAt :: InstructionCounter -> Word64 -> Value
prefixAt counter ran = foldl' (\value (path,replacement) -> wireField path replacement value) localPrefix
  [ (["elapsed","instructions"],A.toJSON (counterValue counter)),(["elapsed","ran"],A.toJSON ran)
  , (["state","instructions"],A.toJSON (counterValue counter))
  ]

sdkMenu :: Value
sdkMenu = object ["menu_id" .= (1::Word32),"setting_id" .= (5::Word32),"cursor" .= (0::Word32)]

warmResult :: PreparedStep -> Value
warmResult step = case stepAction (preparedKind step) of
  Storage.SetModeDial position -> object
    [ "kind" .= ("mode-dial"::T.Text)
    , "dial" .= object ["position" .= modeDialName position,"sample" .= (10::Word16),"low" .= (9::Word16),"high" .= (11::Word16),"menu" .= sdkMenu]
    , "bounded_post_settle_prefix" .= prefixAt (preparedEnd step) (counterValue (preparedEnd step)-counterValue (preparedIdle step))
    ]
  action -> object
    [ "kind" .= ("key"::T.Text)
    , "idle" .= object ["instructions" .= counterValue (preparedIdle step),"ran" .= instructionsValue preKeyIdle]
    , "press" .= object
        [ "snapshot" .= Null,"key" .= key action,"menu" .= sdkMenu
        , "settle" .= object ["status" .= ("packet-consumed"::T.Text)]
        , "instructions" .= (counterValue (preparedEnd step)-counterValue (preparedIdle step)-10)
        , "display_wait_instructions" .= (0::Word64),"failure" .= Null
        ]
    , "bounded_post_settle_prefix" .= prefixAt (preparedEnd step) 10
    ]
  where
    key action = case action of Storage.PressKey value -> T.pack (crawlKeyWire value); Storage.PressHalfShutter -> "half-press"; _ -> error "cold step is not warm"

matrixStepWireBounds :: M.Map FixtureStep ((Value, Value), (Value, Value))
matrixStepWireBounds = M.fromList
  [ (step, ((regionsValue beforeRegions, regionsValue afterRegions), (stateWire before, stateWire after)))
  | step <- [minBound .. RepeatRoundTrips]
  , let (before, after) = stateBounds step
        beforeRegions = stateRegions before
        afterRegions = stateRegions after ]

appliedRegionsValue, rebootedRegionsValue, appliedStateValue, rebootedStateValue :: Value
appliedRegionsValue = regionsValue (stateRegions appliedState)
rebootedRegionsValue = regionsValue (stateRegions rebootedState)
appliedStateValue = stateWire appliedState
rebootedStateValue = stateWire rebootedState

warmStepWire :: PreparedStep -> Value
warmStepWire step = object
  [ "step" .= (case preparedNumber step of StepNumber number -> number),"name" .= stepName kind,"session" .= (0::Word32)
  , "action" .= stepAction kind,"hits" .= [cursorValue (preparedFrom step),cursorValue (preparedTo step)]
  , "watch" .= watch "fixed" (preparedRows step)
  , "regions_before" .= fst regions,"regions_after" .= snd regions
  , "storage_before" .= fst states,"storage_after" .= snd states,"result" .= warmResult step
  ]
  where
    kind=preparedKind step
    (regions,states)=matrixStepWireBounds M.! kind

resetSidecar :: Regions -> [(InstructionCounter,HitCursor,[Value],Regions)] -> B.ByteString
resetSidecar initial phases = encodeWire (object
  [ "schema" .= (1::Int),"reset" .= object ["instructions" .= (0::Word64)],"regions_at_reset" .= regionsValue initial
  , "segments" .= [object
      [ "phase" .= phase,"instructions" .= counterValue counter,"hits_end" .= cursorValue end
      , "watch" .= watch "observe" rows,"regions" .= regionsValue regions
      ] | (phase,(counter,end,rows,regions)) <- zip [minBound..maxBound::AttachPhase] phases]
  ])

initialReset :: [Value] -> B.ByteString
initialReset bootTrace = resetSidecar (stateRegions initialState)
  (replicate 3 (InstructionCounter (100*fromIntegral count),HitCursor count,[],stateRegions initialState))
  where count=length bootTrace

freshLoad :: Role -> WriteWitness
freshLoad role =
  let writes=changedWrites role zeroRegions (stateRegions rebootedState)
  in WriteWitness (readHit role:writeTrace writes) (writeRows writes)

freshReset :: WriteWitness -> B.ByteString
freshReset load = resetSidecar zeroRegions
  [ (InstructionCounter 200000,HitCursor count,stamp (InstructionCounter 100) (writeRows load),stateRegions rebootedState)
  , (InstructionCounter 210000,HitCursor count,[],stateRegions rebootedState)
  , (InstructionCounter 220000,HitCursor count,[],stateRegions rebootedState)
  ]
  where
    count=length (writeTrace load)

roleExportDigest :: Role -> String
roleExportDigest role = digestOf (C.pack ("committed medium for " ++ roleWire role))

bootIdentity :: Role -> BootSession -> Value
bootIdentity role session = object
  [ "schema" .= (3::Int),"model" .= ("synthetic-xm1"::T.Text)
  , "run_document" .= digestOf "run","installed_dat" .= (if role == RetainedRole then sourceDat else ownedDat)
  , "boot_rom" .= digestOf "rom"
  , "adjustment_nand" .= (if session == BootSession 0 then A.toJSON (digestOf "nand") else Null)
  , "record_nand" .= Null,"committed_nand" .= (if session == BootSession 0 then Null else A.toJSON (roleExportDigest role))
  , "source_kind" .= (if session == BootSession 0 then "initial-seeded" else "full-committed"::T.Text)
  , "card_sha256" .= Null,"procedure" .= digestOf "synthetic SDK procedure"
  ]

freshStart :: Role -> BootSession -> B.ByteString -> Value
freshStart role session reset = object
  [ "key" .= digestOf (encodeWire identity),"inputs" .= identity,"directory" .= directory
  , "key_seconds" .= (0::Double),"start" .= ("fresh-boot"::T.Text),"boot_seconds" .= (0::Double),"saved" .= False
  , "reset_observation" .= object ["path" .= (directory <> "/reset-observation.json"),"sha256" .= digestOf reset]
  ]
  where
    identity=bootIdentity role session
    directory=case session of BootSession number -> T.pack ("boot-" ++ replicate (4-length (show number)) '0' ++ show number)

finalSdk :: Role -> BootSession -> B.ByteString -> InstructionCounter -> Value
finalSdk role session reset counter = object
  [ "schema" .= (1::Int),"inputs" .= bootIdentity role session,"checkpoint" .= freshStart role session reset
  , "bridge_error" .= Null,"capture_evidence_error" .= Null
  , "boundary" .= object ["instructions" .= counterValue counter,"failure" .= Null]
  ]

bootManifest :: Role -> BootSession -> B.ByteString -> InstructionCounter -> (HitCursor,HitCursor) -> Value
bootManifest role session reset counter (from,to) = object
  [ "session" .= (case session of BootSession number -> number),"hit_start" .= cursorValue from,"hit_end" .= cursorValue to
  , "sdk" .= finalSdk role session reset counter,"start" .= freshStart role session reset
  , "input_committed_sha256" .= (if session == BootSession 0 then Null else A.toJSON (roleExportDigest role))
  ]

committedExport :: Role -> InstructionCounter -> Value
committedExport role counter = object
  [ "status" .= ("exported"::T.Text)
  , "artifact" .= object ["path" .= ("boot-0000/nand-" <> stepName RebootCommitted <> ".nand.gz"),"sha256" .= roleExportDigest role,"bytes" .= (4096::Word64)]
  , "medium" .= ("type1"::T.Text),"source_kind" .= ("initial-seeded"::T.Text)
  , "base" .= object ["sha256" .= digestOf "synthetic NAND base","bytes" .= (134217728::Word64)]
  , "inputs" .= bootIdentity role (BootSession 0),"boundary" .= boundary,"after" .= boundary
  , "fingerprint" .= digestOf "synthetic NAND fingerprint","format" .= ("type1-nand-committed-image"::T.Text)
  , "geometry" .= object ["blocks" .= (1024::Int),"pages_per_block" .= (64::Int),"page_bytes" .= (2048::Int),"spare_bytes" .= (512::Int)]
  ]
  where boundary=object ["instructions" .= counterValue counter,"clock" .= (0::Word64)]

matrixObservation :: Role -> M.Map FixtureStep TraceMutation -> Observation
matrixObservation role = matrixObservationWithBoot role []

matrixObservationWithBoot :: Role -> [Value] -> M.Map FixtureStep TraceMutation -> Observation
matrixObservationWithBoot = matrixObservationForSetting Storage.FilmSimulation

matrixObservationForSetting :: Storage.StorageSetting -> Role -> [Value] -> M.Map FixtureStep TraceMutation -> Observation
matrixObservationForSetting family role bootTrace changes = Observation
  { obsReceipt=receipt,obsSpec=spec,obsReset=firstReset,obsHits=hits,obsSteps=steps,obsSession=sessionBytes
  , obsBootResets=M.singleton (BootSession 1) childReset
  }
  where
    prepared=prepareSteps (matrixWitnessesFor family role) changes bootTrace
    firstReset=if null bootTrace then matrixInitialReset else initialReset bootTrace
    resetDigest=if null bootTrace then matrixInitialResetDigest else sha256 firstReset
    (oldEnd,oldCounter)=case reverse prepared of
      lastStep:_ -> (preparedTo lastStep,preparedEnd lastStep)
      [] -> error "matrix has no warm steps"
    childReset=if M.member RebootCommitted changes then freshReset load else matrixChildResets M.! role
    load=let original=matrixFreshLoads M.! role
         in original {writeTrace=M.findWithDefault id RebootCommitted changes (writeTrace original)}
    childHits=if M.member RebootCommitted changes then stamp (InstructionCounter 100) (writeTrace load) else matrixChildHits M.! role
    newEnd=HitCursor (cursorValue oldEnd+length childHits)
    childCounter=InstructionCounter 220000
    manifest=object
      [ "schema" .= (1::Int),"kind" .= ("settings-storage-lifecycle"::T.Text)
      , "sessions" .=
          [bootManifest role (BootSession 0) firstReset oldCounter (HitCursor 0,oldEnd)
          ,bootManifest role (BootSession 1) childReset childCounter (oldEnd,newEnd)]
      ]
    sessionBytes=encodeWire manifest
    coldStep=object
      [ "step" .= (fromEnum RebootCommitted+1),"name" .= stepName RebootCommitted,"session" .= (0::Word32)
      , "action" .= Storage.ColdReboot,"hits" .= [cursorValue oldEnd,cursorValue newEnd],"watch" .= watch "fixed" []
      , "regions_before" .= appliedRegionsValue,"regions_after" .= rebootedRegionsValue
      , "storage_before" .= appliedStateValue,"storage_after" .= rebootedStateValue
      , "result" .= object
          [ "kind" .= ("cold-reboot"::T.Text),"export" .= committedExport role oldCounter
          , "prior_session" .= finalSdk role (BootSession 0) firstReset oldCounter
          , "new_start" .= freshStart role (BootSession 1) childReset,"new_reset" .= decodeWire childReset
          , "input_committed_sha256" .= roleExportDigest role,"next_session" .= (1::Word32),"new_hits_start" .= cursorValue oldEnd
          , "regions_before_reboot" .= appliedRegionsValue,"storage_before_reboot" .= appliedStateValue
          ]
      ]
    hits=B.concat [encodeWire value <> "\n" | value <- stamp (InstructionCounter 0) bootTrace ++ concatMap preparedTrace prepared ++ childHits]
    steps=encodeWire (A.toJSON (map warmStepWire prepared ++ [coldStep]))
    spec=matrixSpecFor family role
    same identity=object ["sha256" .= (identity::String)]
    bound digest=object ["sha256" .= sha256Hex digest]
    receipt=encodeWire (object
      [ "role" .= roleWire role
      , "inputs" .= object
          [ "dat" .= same (if role == RetainedRole then sourceDat else ownedDat)
          , "routes" .= bound (matrixWorkloadDigests M.! family)
          , "contract" .= bound matrixContractDigest
          , "linked" .= bound matrixImage4Hash
          , "symbols" .= bound matrixSymbolsDigest
          , "observer" .= same observer
          , "run" .= bound matrixRunDigest
          , "boot_rom" .= bound matrixRomDigest
          , "adjustment_nand" .= bound matrixNandDigest
          , "committed_nand" .= Null
          , "bridge" .= bound matrixBridgeDigest
          , "elf" .= bound matrixElfDigest
          , "analysis" .= matrixAnalysisValue
          , "role_image4_sha256" .= sha256Hex (matrixRoleImageDigests M.! role)
          ]
      , "files" .= object
          [ "spec.json" .= sha256Hex (matrixSpecDigests M.! (family,role))
          , "reset-observation.json" .= sha256Hex resetDigest
          , "hits.jsonl" .= digestOf hits
          , "steps.json" .= digestOf steps
          , "emulator-session.json" .= digestOf sessionBytes
          ]
      , "outcome" .= object ["status" .= ("completed"::T.Text)]
      , "bounds" .= object ["hit_limit" .= hitLimit,"watch_limit" .= watchLimit],"probe" .= Null
      , "pre_key_idle_instructions" .= instructionsValue preKeyIdle,"post_settle_prefix_ticks" .= tickValue postSettle
      ])

mapWireTree :: (Value -> Value) -> Value -> Value
mapWireTree change value = change (case value of
  Object fields -> Object (fmap (mapWireTree change) fields)
  Array values -> Array (fmap (mapWireTree change) values)
  _ -> value)

mapJsonBytes :: (Value -> Value) -> B.ByteString -> B.ByteString
mapJsonBytes change = encodeWire . change . decodeWire

mapHitBytes :: (Value -> Value) -> B.ByteString -> B.ByteString
mapHitBytes change bytes = B.concat [encodeWire (change (decodeWire line)) <> "\n" | line <- C.lines bytes,not (B.null line)]

rebindFiles :: Observation -> Observation
rebindFiles observation = observation {obsReceipt=encodeWire updated}
  where
    updated=foldl' (\value (name,bytes) -> wireField ["files",K.fromText name] (A.toJSON (digestOf bytes)) value)
      (decodeWire (obsReceipt observation))
      [("spec.json",obsSpec observation),("reset-observation.json",obsReset observation)
      ,("hits.jsonl",obsHits observation),("steps.json",obsSteps observation),("emulator-session.json",obsSession observation)]

transformObservation :: (Value -> Value) -> Observation -> Observation
transformObservation change observation = rebindFiles transformed
  where
    newResets=fmap (mapJsonBytes change) (obsBootResets observation)
    resetDigests=[(digestOf before,digestOf after) | (session,before) <- M.toList (obsBootResets observation)
      ,Just after <- [M.lookup session newResets]]
    repair=mapWireTree (\value -> case value of
      String text -> case [after | (before,after) <- resetDigests,text == T.pack before] of
        after:_ -> A.toJSON after
        [] -> value
      _ -> value)
    transformed=observation
      { obsReset=mapJsonBytes change (obsReset observation)
      , obsHits=mapHitBytes change (obsHits observation)
      , obsSteps=mapJsonBytes (repair . change) (obsSteps observation)
      , obsSession=mapJsonBytes (repair . change) (obsSession observation)
      , obsBootResets=newResets
      }

alterObservedStep :: FixtureStep -> (Value -> Value) -> Observation -> Observation
alterObservedStep step change observation = rebindFiles observation {obsSteps=mapJsonBytes update (obsSteps observation)}
  where
    update (Array values) = Array (V.imap (\index value -> if index == fromEnum step then change value else value) values)
    update _ = error "storage steps fixture is not an array"

alterColdResult :: (Value -> Value) -> Observation -> Observation
alterColdResult change = alterObservedStep RebootCommitted update
  where
    update value@(Object fields) = case KM.lookup "result" fields of
      Just result -> wireField ["result"] (change result) value
      Nothing -> error "cold step fixture has no result"
    update _ = error "cold step fixture is not an object"

fixtureArgument :: Int -> Word32 -> Value -> Value
fixtureArgument index value = wireField ["registers",K.fromString ("r" ++ show index)] (A.toJSON (hex value))

hasFixtureSite :: SiteLabel -> Value -> Bool
hasFixtureSite label (Object fields) = KM.lookup "site" fields == Just (A.toJSON (siteLabelWire label))
hasFixtureSite _ _ = False

withoutSite :: SiteLabel -> TraceMutation
withoutSite label = filter (not . hasFixtureSite label)

swapFixtureSites :: SiteLabel -> SiteLabel -> TraceMutation
swapFixtureSites left right values = case ([value | value <- values,hasFixtureSite left value],[value | value <- values,hasFixtureSite right value]) of
  ([leftValue],[rightValue]) -> map (\value -> if hasFixtureSite left value then rightValue else if hasFixtureSite right value then leftValue else value) values
  _ -> error "swap fixture sites are not unique"

programBytes :: B.ByteString
programBytes = recordData sealedRecord <> B.replicate (0x600-52) 0xff


programMutation :: Int -> TraceMutation
programMutation index = alterFirstSite RecordProgram (wireMemory [flippedByte index programBytes])

corruptRecordSeal :: Observation -> Observation
corruptRecordSeal = transformObservation (mapWireTree corrupt)
  where
    rewrite text=foldl' (\result record -> T.replace (wireBytes (recordData record))
      (wireBytes (replaceBytes 0 (B.singleton 0) (recordData record))) result) text [sealedRecord,savedWorking]
    corrupt (String text) = String (rewrite text)
    corrupt value@(Object fields)
      | KM.lookup "source" fields == Just (String "cpu-write")
      , KM.lookup "physical" fields `elem` map (Just . A.toJSON) ([0x89f838,0x89f874]::[Word32])
      , KM.lookup "value" fields == Just (Number 8) = wireField ["value"] (Number 0) value
      | hasFixtureSite (AccessorStore CustomByte) value
      , Just (Object registers) <- KM.lookup "registers" fields
      , KM.lookup "r0" registers `elem` map (Just . A.toJSON . hex) ([0x89f838,0x89f874]::[Word32])
      , KM.lookup "r1" registers == Just (A.toJSON (hex 8)) = fixtureArgument 1 0 value
    corrupt value = value

retainedMatrix,ownedMatrix :: Observation
retainedMatrix=matrixObservation RetainedRole M.empty
ownedMatrix=matrixObservation OwnedRole M.empty

qualifiedMatrix :: Role -> Observation -> Either Refusal Qualified
qualifiedMatrix=qualify matrixAdmitted

outcomeDescription :: Either Refusal a -> String
outcomeDescription result=case result of Left refusal -> " (" ++ show refusal ++ ")"; Right _ -> " (accepted)"

data ComparisonExpectation = RolesAgree | RolesDiffer | StorageEvidenceDiffers

comparisonCase :: String -> ComparisonExpectation -> Observation -> Observation -> (String,Bool)
comparisonCase = comparisonCaseWith matrixAdmitted

comparisonCaseWith :: AdmittedBindings -> String -> ComparisonExpectation -> Observation -> Observation -> (String,Bool)
comparisonCaseWith bound label expectation retained owned = case (qualify bound RetainedRole retained,qualify bound OwnedRole owned) of
  (Right retainedProof,Right ownedProof) ->
    let compared=compareRoles retainedProof ownedProof
        expected=case expectation of
          RolesAgree -> not (isLeft compared)
          RolesDiffer -> isLeft compared
          StorageEvidenceDiffers -> case compared of Left StorageStepsDiffer -> True; _ -> False
    in (label ++ outcomeDescription compared,expected)
  (Left refusal,_) -> (label ++ " retained qualification: " ++ show refusal,False)
  (_,Left refusal) -> (label ++ " owned qualification: " ++ show refusal,False)

refusalCase :: String -> Role -> Observation -> (String,Bool)
refusalCase label role observation =
  let result=qualifiedMatrix role observation
  in (label ++ outcomeDescription result,isLeft result)

stepRefusalCase :: String -> FixtureStep -> TraceMutation -> (String,Bool)
stepRefusalCase label step change = refusalCase label OwnedRole (matrixObservation OwnedRole (M.singleton step change))

matrixConsumerCases :: [(String,Bool)]
matrixConsumerCases =
  [ let result=qualifiedMatrix role observation
    in ("original lifecycle without EE observations qualifies " ++ roleWire role ++ outcomeDescription result,not (isLeft result))
  | (role,observation) <- [(RetainedRole,retainedMatrix),(OwnedRole,ownedMatrix)]
  ] ++
  [ comparisonCase "complete lifecycle compares with distinct role-local committed SHA chains" RolesAgree retainedMatrix ownedMatrix
  , stepRefusalCase "changed refresh requires successful program status" SaveChanged
      (alterFirstSite RecordProgramResult (fixtureArgument 0 1))
  , stepRefusalCase "changed refresh requires a completed medium operation" SaveChanged (withoutSite RecordProgramResult)
  , stepRefusalCase "changed refresh cannot report success before programming" SaveChanged (swapFixtureSites RecordProgram RecordProgramResult)
  , stepRefusalCase "changed refresh requires its actual append request" SaveChanged (withoutSite Area3Append)
  , stepRefusalCase "changed refresh requires actual return" SaveChanged (withoutSite (StorageSite Storage.CustomRefreshReturn))
  , stepRefusalCase "unchanged save requires an invocation rather than absence of NV events" SaveUnchanged
      (withoutSite (StorageSite Storage.CustomRefreshEntry) . withoutSite (StorageSite Storage.CustomRefreshReturn))
  , stepRefusalCase "unchanged refresh cannot fail" SaveUnchanged
      (alterFirstSite (StorageSite Storage.CustomRefreshReturn) (fixtureArgument 0 1))
  , stepRefusalCase "alternate cancellation requires restore direction" RestoreAlternate
      (alterFirstSite (StorageSite Storage.ExchangeAlternateEntry) (fixtureArgument 0 0))
  , stepRefusalCase "alternate capture compares all 450 bytes" CaptureAlternate
      (alterFirstSite (StorageSite Storage.ExchangeAlternateReturn)
        (wireMemory [bankBytes activeA,flippedByte 449 (bankBytes activeA),B.replicate 450 0,bankBytes backingA]))
  , stepRefusalCase "alternate restore requires the captured image" RestoreAlternate
      (alterFirstSite (StorageSite Storage.ExchangeAlternateEntry)
        (wireMemory [bankBytes activeB,flippedByte 449 (bankBytes activeA),B.replicate 450 0,bankBytes backingB]))
  , stepRefusalCase "alternate restore compares all 280 backing bytes" RestoreAlternate
      (alterFirstSite (StorageSite Storage.ExchangeAlternateReturn)
        (wireMemory [bankBytes activeA,bankBytes activeA,B.replicate 450 0,flippedByte 279 (bankBytes restoredBacking)]))
  , stepRefusalCase "C selection requires record zero" SelectCustom (alterFirstSite GetterEntry (fixtureArgument 3 1))
  , stepRefusalCase "live selection requires record one" SelectLive (alterFirstSite GetterEntry (fixtureArgument 3 0))
  , stepRefusalCase "C getter requires its actual default-getter caller" SelectCustom
      (alterFirstSite GetterEntry (wireField ["lr"] (A.toJSON (hex 0x62464c))))
  , stepRefusalCase "repeat film requires the actual property" RepeatRoundTrips (alterFirstSite SetterEntry (fixtureArgument 0 0xfd))
  , stepRefusalCase "repeat film requires assignment success" RepeatRoundTrips (alterFirstSite SetterReturn (fixtureArgument 0 0))
  , stepRefusalCase "repeat film requires matching readback" RepeatRoundTrips (alterFirstSite GetterReturn (fixtureArgument 0 4))
  , stepRefusalCase "cold reload requires an actual record read" RebootCommitted (withoutSite RecordRead)
  , stepRefusalCase "cold reload refuses an unowned read output" RebootCommitted (alterFirstSite RecordRead (fixtureArgument 1 0x8a0081))
  , stepRefusalCase "owned refresh cannot execute the fixed veneer" SaveChanged
      (alterFirstSite (StorageSite Storage.CustomRefreshEntry) (wireField ["address"] (A.toJSON (hex 0x6a86c4))))
  , stepRefusalCase "custom writes cannot cross the owned working-record guard" SaveChanged
      (alterFirstSite (AccessorStore CustomByte) (fixtureArgument 0 0x89f873))
  , refusalCase "C class must agree with actual scanner bit25" OwnedRole
      (alterObservedStep SelectCustom (wireField ["storage_after","c_input_bitmap"] (Number 0)) ownedMatrix)
  , refusalCase "C snapshot must carry class three" OwnedRole
      (alterObservedStep SelectCustom (wireField ["storage_after","mode_class"] (Number 0)) ownedMatrix)
  , comparisonCase "all 600 program bytes remain role-comparable" RolesDiffer retainedMatrix
      (matrixObservation OwnedRole (M.singleton SaveChanged (programMutation 0x5ff)))
  , let result=qualifiedMatrix OwnedRole (corruptRecordSeal ownedMatrix)
    in ("consistent malformed HW3 record is not a changed-save witness" ++ outcomeDescription result
      ,case result of Left (StorageCheckFailed _ _) -> True; _ -> False)
  ] ++
  [ stepRefusalCase ("changed program rejects modified record byte " ++ show index) SaveChanged (programMutation index)
  | index <- [0,1,2,3,4,43,44,45,47,48,51]
  ] ++
  [stepRefusalCase label HalfPressApplication change | (label,change) <- s1Mutations] ++
  [comparisonCase label StorageEvidenceDiffers retainedMatrix (matrixObservation OwnedRole (M.singleton HalfPressApplication change))
  | (label,change) <- s1ComparisonMutations] ++
  coldProvenanceCases ++ coverageCases ++ matrixControlCases ++ optionalEeCases ++ repetitionFamilyCases ++ [fullImageProofBindingCase]

mapStorageSnapshots :: (Value -> Value) -> Observation -> Observation
mapStorageSnapshots change = transformObservation (mapWireTree update)
  where
    update value@(Object fields) | KM.member "c_scanner_row" fields = change value
    update value = value

semanticRefusalCase :: String -> Observation -> (String,Bool)
semanticRefusalCase label observation =
  let result=qualifiedMatrix OwnedRole observation
  in (label ++ outcomeDescription result,case result of Left (StorageCheckFailed _ _) -> True; _ -> False)

changedInitialSeed :: Observation
changedInitialSeed = rebindFiles changed {obsReceipt=encodeWire receipt}
  where
    oldIdentity=bootIdentity OwnedRole (BootSession 0)
    newIdentity=wireField ["adjustment_nand"] (A.toJSON newSeed) oldIdentity
    newSeed=digestOf "different initial NAND"
    oldKey=digestOf (encodeWire oldIdentity)
    newKey=digestOf (encodeWire newIdentity)
    change value
      | value == oldIdentity = newIdentity
      | value == A.toJSON oldKey = A.toJSON newKey
      | otherwise = value
    changed=transformObservation (mapWireTree change) ownedMatrix
    receipt=wireField ["inputs","adjustment_nand","sha256"] (A.toJSON newSeed) (decodeWire (obsReceipt changed))

reseededChild :: Observation
reseededChild = transformObservation (mapWireTree change) ownedMatrix
  where
    oldIdentity=bootIdentity OwnedRole (BootSession 1)
    newIdentity=wireField ["source_kind"] (String "initial-seeded")
      (wireField ["committed_nand"] Null (wireField ["adjustment_nand"] (A.toJSON (digestOf "nand")) oldIdentity))
    oldKey=digestOf (encodeWire oldIdentity)
    newKey=digestOf (encodeWire newIdentity)
    change value
      | value == oldIdentity = newIdentity
      | value == A.toJSON oldKey = A.toJSON newKey
      | otherwise = value

coldProvenanceCases :: [(String,Bool)]
coldProvenanceCases =
  [ refusalCase "cold result binds the actual closed parent SDK document" OwnedRole
      (alterColdResult (wireField ["prior_session","boundary","instructions"] (Number 0)) ownedMatrix)
  , refusalCase "cold result binds the actual fresh SDK child" OwnedRole
      (alterColdResult (wireField ["new_start","directory"] (String "unrelated-boot")) ownedMatrix)
  , refusalCase "cold result cannot borrow another role's committed export" OwnedRole
      (alterColdResult (wireField ["input_committed_sha256"] (A.toJSON (roleExportDigest RetainedRole))) ownedMatrix)
  , refusalCase "cold export source kind must match its actual SDK inputs" OwnedRole
      (alterColdResult (wireField ["export","source_kind"] (String "full-committed")) ownedMatrix)
  , refusalCase "cold export cannot claim an earlier SDK boundary" OwnedRole
      (alterColdResult (wireField ["export","boundary","instructions"] (Number 0)
        . wireField ["export","after","instructions"] (Number 0)) ownedMatrix)
  , refusalCase "cold export geometry must be the actual full committed medium" OwnedRole
      (alterColdResult (wireField ["export","geometry","page_bytes"] (Number 4096)) ownedMatrix)
  , refusalCase "fresh committed child cannot be relabeled as an adjustment seed" OwnedRole reseededChild
  , refusalCase "raw child reset SHA is byte-bound even for equivalent JSON" OwnedRole
      ownedMatrix {obsBootResets=M.adjust (" " <>) (BootSession 1) (obsBootResets ownedMatrix)}
  , refusalCase "inline child reset must equal its raw reset sidecar" OwnedRole
      (alterColdResult (wireField ["new_reset","reset","instructions"] (Number 1)) ownedMatrix)
  , refusalCase "child reset phases cannot advance beyond the SDK boundary" OwnedRole
      (transformObservation (mapWireTree laterReset) ownedMatrix)
  , refusalCase "fresh child cannot load a saved emulator checkpoint" OwnedRole
      (transformObservation (mapWireTree savedCheckpoint) ownedMatrix)
  , comparisonCase "initial adjustment NAND remains a cross-role cohort input" RolesDiffer retainedMatrix changedInitialSeed
  , semanticRefusalCase "C scanner bit25 is an observed selection requirement"
      (mapStorageSnapshots (wireField ["c_input_bitmap"] (Number 0)) ownedMatrix)
  , semanticRefusalCase "C mode class is an observed selection requirement"
      (mapStorageSnapshots changeCustomClass ownedMatrix)
  , refusalCase "a completed handback before half-press does not establish causal application" OwnedRole
      (matrixObservation OwnedRole (M.fromList
        [(RestoreAlternate,(++ s1ApplicationHits OwnedRole)),(HalfPressApplication,const [])]))
  ]
  where
    laterReset value@(Object fields)
      | KM.lookup "phase" fields == Just (String "first-start")
      , KM.lookup "instructions" fields == Just (Number 200000) = wireField ["instructions"] (Number 230000) value
    laterReset value = value
    savedCheckpoint value@(Object fields)
      | KM.lookup "start" fields == Just (String "fresh-boot") = wireField ["saved"] (Bool True) value
    savedCheckpoint value = value
    changeCustomClass value@(Object fields)
      | KM.lookup "mode_class" fields == Just (Number 3) = wireField ["mode_class"] (Number 0) value
    changeCustomClass value = value

coverageCases :: [(String,Bool)]
coverageCases =
  [ let workload=encodeWire (case decodeWire matrixWorkload of
          Object fields -> case KM.lookup "steps" fields of
            Just (Array steps) -> Object (KM.insert "steps" (Array (fmap clearChecks steps)) fields)
            _ -> error "matrix workload has no steps"
          _ -> error "matrix workload is not an object")
        bindings=matrixBindings {bindRoutes=workload}
        admitted=either (error . show) id (admitBindings bindings)
        observation=ownedMatrix {obsReceipt=mapJsonBytes
          (wireField ["inputs","routes","sha256"] (A.toJSON (digestOf workload))) (obsReceipt ownedMatrix)}
        result=qualify admitted OwnedRole observation
    in ("executing the full workload without its required semantic checks earns no coverage" ++ outcomeDescription result
      ,case result of Left (StorageCoverageMissing _) -> True; _ -> False)
  ]
  where
    clearChecks=wireField ["checks"] (Array V.empty)

controlObservation :: Role -> Observation
controlObservation role = matrixObservationWithBoot role boot (M.singleton EditAlternate (++ getterRun))
  where
    boot=
      [matrixHit role SetterEntry setterEntry [0xfe,2,0,1] 0 (matrixStack role) (setterCall+4) []
      ,matrixHit role SetterReturn (setterCall+4) [1] 0 (matrixStack role) 0 []]
    getterRun=concat
      [ [matrixHit role GetterEntry getterEntry [0xfe,0,0,1] 0 (matrixStack role) (getterCall+4) []
        ,matrixHit role GetterReturn (getterCall+4) [value] 0 (matrixStack role) 0 []]
      | value <- [3,3,3,4]]

retainedControlMatrix,ownedControlMatrix :: Observation
retainedControlMatrix=controlObservation RetainedRole
ownedControlMatrix=controlObservation OwnedRole

matrixSemanticRefusal :: Refusal -> Bool
matrixSemanticRefusal refusal=case refusal of
  StorageShape _ -> True
  RegionContinuity _ -> True
  StorageCheckFailed _ _ -> True
  StorageStepsDiffer -> True
  _ -> False

controlRefusal :: Role -> ControlName -> Refusal -> Bool
controlRefusal role control refusal=case control of
  WrongDatDigest -> inputRefusal
  WrongLinkedBin -> inputRefusal
  InjectedGuardedFetch -> case (role,refusal) of
    (RetainedRole,UnarmedSite _) -> True
    (OwnedRole,GuardedFetch _) -> True
    _ -> False
  MissingSetterCall -> case refusal of SetterSequenceDiffers _ -> True; _ -> matrixSemanticRefusal refusal
  ChangedGetterResult -> getterRefusal
  StaleGetterResult -> getterRefusal
  ReorderedWrite -> writeRefusal
  DroppedWrite -> writeRefusal
  ExtraWrite -> writeRefusal
  ChangedValue -> writeRefusal
  where
    inputRefusal=case refusal of InputDigest _ -> True; _ -> False
    getterRefusal=case refusal of GetterSequenceDiffers _ _ -> True; _ -> matrixSemanticRefusal refusal
    writeRefusal=case refusal of
      WriteSequenceDiffers _ -> True
      AccessorStoreUnmatched _ -> role == OwnedRole
      _ -> matrixSemanticRefusal refusal

matrixControlCases :: [(String,Bool)]
matrixControlCases =
  comparisonCase "complete control cohort qualifies before any mutation" RolesAgree retainedControlMatrix ownedControlMatrix :
  [ let (mine,other)=if role == RetainedRole then (retainedControlMatrix,ownedControlMatrix) else (ownedControlMatrix,retainedControlMatrix)
        label="public matrix control " ++ roleWire role ++ " " ++ show control
    in case runControl matrixAdmitted role mine other control of
      Left prerequisite -> (label ++ " prerequisite: " ++ prerequisite,False)
      Right outcome ->
        (label ++ outcomeDescription (controlObserved outcome)
        ,controlName outcome == control
          && case controlObserved outcome of
            Left refusal -> controlRefusal role control refusal
              && case control of
                StaleGetterResult -> case controlStale outcome of
                  Just _ -> True
                  Nothing -> False
                _ -> True
            Right () -> False)
  | role <- [RetainedRole,OwnedRole],control <- controlNames]

fullImageProofBindingCase :: (String,Bool)
fullImageProofBindingCase =
  ("an identical bank body cannot reuse proof from a different full linked image"
  ,case (composedInputs,fixturePlanInputs OwnedRole matrixBindings) of
    (Right original,Right expanded) -> not (isLeft (deriveStoragePlan expanded matrixDefinition))
      && isLeft (deriveStoragePlan expanded {inputBankBoundary=inputBankBoundary original} matrixDefinition)
    _ -> False)

data SerializedEeBranch = EqualEeBranch | DifferentEeBranches

optionalEeTrace :: Role -> [Value] -> Observation
optionalEeTrace role values = matrixObservation role (M.singleton RepeatRoundTrips (++ values))

optionalEeObservation :: Role -> SerializedEeBranch -> TraceMutation -> Observation
optionalEeObservation role branch change = optionalEeTrace role (change values)
  where
    values=case branch of
      EqualEeBranch -> eeEqualHits role (activeImage appliedState)
      DifferentEeBranches -> eeDifferentHits role (activeImage appliedState)

optionalEeRefusalCase :: String -> SerializedEeBranch -> TraceMutation -> (String,Bool)
optionalEeRefusalCase label branch change = refusalCase label OwnedRole (optionalEeObservation OwnedRole branch change)

optionalEeCases :: [(String,Bool)]
optionalEeCases =
  [ comparisonCase "optional observed equal EE path compares" RolesAgree
      (optionalEeObservation RetainedRole EqualEeBranch id) (optionalEeObservation OwnedRole EqualEeBranch id)
  , comparisonCase "optional observed different EE and class37 paths compare" RolesAgree
      (optionalEeObservation RetainedRole DifferentEeBranches id) (optionalEeObservation OwnedRole DifferentEeBranches id)
  , optionalEeRefusalCase "observed owned bank application cannot execute the fixed veneer" DifferentEeBranches
      (alterFirstSite (StorageSite Storage.BankApplyEntry) (wireField ["address"] (A.toJSON (hex 0x6309c4))))
  , comparisonCase "optional observed EE compares every bank byte" StorageEvidenceDiffers
      (optionalEeObservation RetainedRole EqualEeBranch id)
      (optionalEeObservation OwnedRole EqualEeBranch
        (alterFirstSite (StorageSite Storage.EeCompare) (wireMemory
          [flippedByte 449 (bankBytes (activeImage appliedState)),littleWords [0],littleWords [2],littleWords [30]])))
  ] ++
  [ comparisonCase (label ++ " with retained7ffffffe metadata") RolesAgree
      (optionalEeTrace RetainedRole (build RetainedRole (activeImage appliedState)))
      (optionalEeTrace OwnedRole (build OwnedRole (activeImage appliedState)))
  | (label,build) <- eeConversionCases
  ] ++
  [optionalEeRefusalCase label EqualEeBranch change | (label,change) <- eeEqualMutations] ++
  [optionalEeRefusalCase label DifferentEeBranches change | (label,change) <- eeDifferentMutations]

repetitionFamilyCases :: [(String,Bool)]
repetitionFamilyCases =
  [ comparisonCaseWith familyAdmitted "white-balance repetition is an optional choice of repeated-setting family" RolesAgree
      (matrixObservationForSetting Storage.WhiteBalanceShift RetainedRole [] M.empty)
      (matrixObservationForSetting Storage.WhiteBalanceShift OwnedRole [] M.empty)
  , let result=qualify familyAdmitted OwnedRole
          (matrixObservationForSetting Storage.WhiteBalanceShift OwnedRole []
            (M.singleton RepeatRoundTrips (alterFirstSite GetterEntry (fixtureArgument 3 0))))
    in ("optional white-balance repetition still requires matching readback record" ++ outcomeDescription result
      ,case result of Left (StorageCheckFailed _ _) -> True; _ -> False)
  ]
  where familyAdmitted=matrixAdmittedFor Storage.WhiteBalanceShift
