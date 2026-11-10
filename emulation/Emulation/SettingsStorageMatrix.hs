{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}
module Emulation.SettingsStorageMatrix
  ( StorageMatrix(..), MatrixSource(..), SourceCommit, sourceCommitHex
  , StorageStep(..), MatrixStepName, matrixStepNameText
  , StorageAction(..), StorageCheck(..), RecordChange(..), ImageFamily(..), CopyDirection(..)
  , PayloadRelation(..), PayloadTarget(..), PayloadReference(..), ProfileSlot(..), StorageSetting(..)
  , LifecycleSite(..), lifecycleSiteWire, lifecycleSites, lifecycleWords, lifecycleReads
  , decodeStorageMatrix, storageMatrixValue, storageActionValue, storageCheckValue
  ) where

import Control.Monad (unless, forM_)
import Data.Aeson (FromJSON(..), ToJSON(..), Value(..), eitherDecodeStrict', object, withObject, (.:), (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (Object, Parser)
import qualified Data.ByteString as B
import Data.Char (isHexDigit)
import qualified Data.Set as S
import qualified Data.Text as T
import Data.Word (Word32)
import Emulation.Sha256 (Sha256, parseSha256Hex, sha256Hex)
import EmulatorUsb (CrawlKey(..), ModeDial(..), crawlKeyWire, modeDialName)

newtype SourceCommit = SourceCommit String deriving Eq
sourceCommitHex :: SourceCommit -> String
sourceCommitHex (SourceCommit value) = value
newtype MatrixStepName = MatrixStepName T.Text deriving (Eq, Ord, Show)
matrixStepNameText :: MatrixStepName -> T.Text
matrixStepNameText (MatrixStepName value) = value

data MatrixSource = MatrixSource
  { matrixDecompCommit :: SourceCommit
  , matrixImage4 :: Sha256
  , matrixImage5 :: Sha256
  , matrixCatalogCommit :: SourceCommit
  , matrixControlsCommit :: SourceCommit
  } deriving Eq

data StorageMatrix = StorageMatrix { matrixSource :: MatrixSource, matrixSteps :: [StorageStep] } deriving Eq
data StorageStep = StorageStep
  { storageStepName :: MatrixStepName
  , storageStepAction :: StorageAction
  , storageStepChecks :: [StorageCheck]
  } deriving Eq
data StorageAction = PressKey CrawlKey | PressHalfShutter | SetModeDial ModeDial | ColdReboot deriving (Eq, Show)
data RecordChange = Changed | Unchanged deriving (Eq, Show)
data ImageFamily = PreviousImage | AlternateImage deriving (Eq, Ord, Show)
data CopyDirection = Capture | Restore deriving (Eq, Ord, Show)
data PayloadRelation = SameAs | DifferentFrom deriving (Eq, Show)
data PayloadTarget = ActiveCustomPayload | BackingCustomPayload | PreviousRecordPayload | WorkingRecordPayload deriving (Eq, Show)
data PayloadReference = PayloadReference { payloadReferenceStep :: MatrixStepName, payloadReferenceTarget :: PayloadTarget } deriving (Eq, Show)
data ProfileSlot = StoredCustom | LiveProfile deriving (Eq, Show)
data StorageSetting = FilmSimulation | WhiteBalanceShift deriving (Eq, Ord, Show)
data StorageCheck
  = CustomCommit RecordChange
  | BankCopy ImageFamily CopyDirection
  | CustomPayload PayloadRelation PayloadTarget PayloadReference
  | CurrentSlot ProfileSlot
  | UiHandback
  | RepeatSetting StorageSetting
  | ColdBootRecord MatrixStepName
  deriving (Eq, Show)

data LifecycleSite
  = BankApplyEntry | BankApplyReturn | CustomRefreshEntry | CustomRefreshReturn
  | CapturePreviousEntry | CapturePreviousReturn | RestorePreviousEntry | RestorePreviousReturn
  | ExchangeAlternateEntry | ExchangeAlternateReturn
  | EeCachedCall | EeFrameClassCall | EeFrameClassResult | EeFramePropertyCall | EeFramePropertyResult
  | EeOldValueCall | EeCompare | EeConditional
  | EeResetZeroLive | EeResetEightLive | EeResetNineLive | EeResetZeroCustom | EeResetEightCustom | EeResetNineCustom
  | EeApplyFirst | EeApplySecond | EeFinalWrite | EeFinalReturn
  | S1ActionEntry | S1GuardCall | S1GuardResult | S1RequestCall | S1DispatchCall | S1DispatchResult
  | S1FlagCall | S1FlagReturn | S1ReplayCall | S1ReplayReturn | S1ActionReturnInstruction | S1ActionReturned
  | S1FrameCopyCall | S1FrameCopyReturn
  | S1DequeueEntry | S1DequeueCopyCall | S1DequeueCopyReturn | S1DequeueSuccess
  | S1DequeueReturnInstruction | S1DequeueWrapperReturn | S1DequeueReturned
  | S1ApplicationActionEntry | S1ApplicationGuardCall | S1ApplicationGuardResult
  | S1ApplicationRouterCall | S1ApplicationRouterReturn | S1ApplicationKeyTest | S1ApplicationQualifierTest
  | S1ApplicationEventStore | S1ApplicationDispatch
  deriving (Eq, Ord, Show, Enum, Bounded)

lifecycleSiteWire :: LifecycleSite -> String
lifecycleSiteWire site = "ss-" ++ case site of
  BankApplyEntry -> "bank-apply-entry"
  BankApplyReturn -> "bank-apply-return"
  CustomRefreshEntry -> "custom-refresh-entry"
  CustomRefreshReturn -> "custom-refresh-return"
  CapturePreviousEntry -> "capture-previous-entry"
  CapturePreviousReturn -> "capture-previous-return"
  RestorePreviousEntry -> "restore-previous-entry"
  RestorePreviousReturn -> "restore-previous-return"
  ExchangeAlternateEntry -> "exchange-alternate-entry"
  ExchangeAlternateReturn -> "exchange-alternate-return"
  EeCachedCall -> "ee-cached-call"
  EeFrameClassCall -> "ee-frame-class-call"
  EeFrameClassResult -> "ee-frame-class-result"
  EeFramePropertyCall -> "ee-frame-property-call"
  EeFramePropertyResult -> "ee-frame-property-result"
  EeOldValueCall -> "ee-old-value-call"
  EeCompare -> "ee-compare"
  EeConditional -> "ee-conditional"
  EeResetZeroLive -> "ee-reset-zero-live"
  EeResetEightLive -> "ee-reset-eight-live"
  EeResetNineLive -> "ee-reset-nine-live"
  EeResetZeroCustom -> "ee-reset-zero-custom"
  EeResetEightCustom -> "ee-reset-eight-custom"
  EeResetNineCustom -> "ee-reset-nine-custom"
  EeApplyFirst -> "ee-apply-first"
  EeApplySecond -> "ee-apply-second"
  EeFinalWrite -> "ee-final-write"
  EeFinalReturn -> "ee-final-return"
  S1ActionEntry -> "s1-action-entry"
  S1GuardCall -> "s1-guard-call"
  S1GuardResult -> "s1-guard-result"
  S1RequestCall -> "s1-request-call"
  S1DispatchCall -> "s1-dispatch-call"
  S1DispatchResult -> "s1-dispatch-result"
  S1FlagCall -> "s1-flag-call"
  S1FlagReturn -> "s1-flag-return"
  S1ReplayCall -> "s1-replay-call"
  S1ReplayReturn -> "s1-replay-return"
  S1ActionReturnInstruction -> "s1-action-return-instruction"
  S1ActionReturned -> "s1-action-returned"
  S1FrameCopyCall -> "s1-frame-copy-call"
  S1FrameCopyReturn -> "s1-frame-copy-return"
  S1DequeueEntry -> "s1-dequeue-entry"
  S1DequeueCopyCall -> "s1-dequeue-copy-call"
  S1DequeueCopyReturn -> "s1-dequeue-copy-return"
  S1DequeueSuccess -> "s1-dequeue-success"
  S1DequeueReturnInstruction -> "s1-dequeue-return-instruction"
  S1DequeueWrapperReturn -> "s1-dequeue-wrapper-return"
  S1DequeueReturned -> "s1-dequeue-returned"
  S1ApplicationActionEntry -> "s1-application-action-entry"
  S1ApplicationGuardCall -> "s1-application-guard-call"
  S1ApplicationGuardResult -> "s1-application-guard-result"
  S1ApplicationRouterCall -> "s1-application-router-call"
  S1ApplicationRouterReturn -> "s1-application-router-return"
  S1ApplicationKeyTest -> "s1-application-key-test"
  S1ApplicationQualifierTest -> "s1-application-qualifier-test"
  S1ApplicationEventStore -> "s1-application-event-store"
  S1ApplicationDispatch -> "s1-application-dispatch"

lifecycleSites :: [(Word32,LifecycleSite)]
lifecycleSites =
  [(0x61db3c,CapturePreviousEntry),(0x61c8f8,CapturePreviousReturn),(0x61db18,RestorePreviousEntry),(0x61c908,RestorePreviousReturn)
  ,(0x61daa8,ExchangeAlternateEntry),(0x61c918,ExchangeAlternateReturn)
  ,(0xd50c38,EeCachedCall),(0xd50c3c,EeFrameClassCall),(0xd50c40,EeFrameClassResult),(0xd50c48,EeFramePropertyCall),(0xd50c4c,EeFramePropertyResult)
  ,(0xd50c5c,EeOldValueCall),(0xd50c60,EeCompare),(0xd50c64,EeConditional)
  ,(0xd50c74,EeResetZeroLive),(0xd50c84,EeResetEightLive),(0xd50c94,EeResetNineLive)
  ,(0xd50ca4,EeResetZeroCustom),(0xd50cb4,EeResetEightCustom),(0xd50cc4,EeResetNineCustom)
  ,(0xd50cd8,EeApplyFirst),(0xd50ce4,EeApplySecond),(0xd50cf4,EeFinalWrite),(0xd50cf8,EeFinalReturn)
  ,(0xc77b90,S1ActionEntry),(0xc77b9c,S1GuardCall),(0xc77ba0,S1GuardResult),(0xc77bb4,S1RequestCall)
  ,(0xc77bb8,S1DispatchCall),(0xc77bbc,S1DispatchResult),(0xc77be4,S1FlagCall),(0xc77be8,S1FlagReturn)
  ,(0xc77bec,S1ReplayCall),(0xc77bf0,S1ReplayReturn),(0xc77bf4,S1ActionReturnInstruction),(0xbfb0cc,S1ActionReturned)
  ,(0xd34ed0,S1FrameCopyCall),(0xd34ed4,S1FrameCopyReturn)
  ,(0xd34eec,S1DequeueEntry),(0xd34f68,S1DequeueCopyCall),(0xd34f6c,S1DequeueCopyReturn),(0xd34fa8,S1DequeueSuccess)
  ,(0xd34fc8,S1DequeueReturnInstruction),(0xd3ab20,S1DequeueWrapperReturn),(0xc23104,S1DequeueReturned)
  ,(0xc24994,S1ApplicationActionEntry),(0xc2499c,S1ApplicationGuardCall),(0xc249a0,S1ApplicationGuardResult)
  ,(0xc249b0,S1ApplicationRouterCall),(0xc249b4,S1ApplicationRouterReturn),(0xc09db8,S1ApplicationKeyTest),(0xc09dcc,S1ApplicationQualifierTest)
  ,(0xc09de0,S1ApplicationEventStore),(0xc09de8,S1ApplicationDispatch)]

lifecycleWords :: [(Word32,Word32)]
lifecycleWords =
  [(0x61c8f8,0xe8bd8800),(0x61c908,0xe8bd8800),(0x61c918,0xe8bd8800)
  ,(0xd50c38,0xebe38b53),(0xd50c3c,0xebe33144),(0xd50c40,0xe3500025),(0xd50c48,0xebe33169),(0xd50c4c,0xe35000ee)
  ,(0xd50c5c,0xebe32f36),(0xd50c60,0xe1500008),(0xd50c64,0x0a00001f)
  ,(0xd50c74,0xebe38e75),(0xd50c84,0xebe38e71),(0xd50c94,0xebe38e6d)
  ,(0xd50ca4,0xebe38e69),(0xd50cb4,0xebe38e65),(0xd50cc4,0xebe38e61)
  ,(0xd50cd8,0xebe3d029),(0xd50ce4,0xebe3d066),(0xd50cf4,0xebe32f08),(0xd50cf8,0xea00065d)
  ,(0xc77b90,0xe92d4818),(0xc77b9c,0xebe70955),(0xc77ba0,0xe3500000),(0xc77bb4,0xeb031727)
  ,(0xc77bb8,0xebfe6bbb),(0xc77bbc,0xe1a04000),(0xc77be4,0xebe6ffc4),(0xc77be8,0xe3a00000)
  ,(0xc77bc0,0xe3500002),(0xc77bc4,0x13700001),(0xc77bc8,0x0a000004),(0xc77bcc,0xe350005d)
  ,(0xc77bd0,0x1350005e),(0xc77bd4,0x0a000001),(0xc77bd8,0xe35000d9),(0xc77bdc,0x1a000005)
  ,(0xc77bec,0xeb030a98),(0xc77bf0,0xe1a00004),(0xc77bf4,0xe8bd8818),(0xbfb0cc,0xe1a04000)
  ,(0xd34ed0,0xebe7b45f),(0xd34ed4,0xe19430b5)
  ,(0xd34eec,0xe92d4810),(0xd34f68,0xebe7b439),(0xd34f6c,0xe30d0510),(0xd34fa8,0xe3a00001)
  ,(0xd34fc8,0xe8bd8810),(0xd3ab20,0xe8bd8800),(0xc23104,0xe3500001)
  ,(0xc24994,0xe92d4818),(0xc2499c,0xeb045735),(0xc249a0,0xe3500000)
  ,(0xc249b0,0xebff94db),(0xc249b4,0xe1a04000),(0xc09db8,0xe3530007),(0xc09dcc,0xe3530000)
  ,(0xc09de0,0xe1c329b4),(0xc09de8,0xebffc48e)]
  ++ eeConversionWords

eeConversionWords :: [(Word32,Word32)]
eeConversionWords =
  [(0x81b454,0x775ca4),(0xd50c34,0xe1a00004)]
  ++ zip [0x775ca4,0x775ca8..] (concat [[code,0x7ffffffe,30-code] | code <- [0..30]] ++ replicate 3 0xffff)
  ++ zip [0x61d514,0x61d518..] [0xe92d4800,0xe28db004,0xeb005137,0xe8bd8800]
  ++ zip [0x63398c,0x633990..]
    [0xe52db004,0xe28db000,0xe3063838,0xe3403095,0xe5830030,0xe28bd000,0xe8bd0800,0xe12fff1e]
  ++ zip [0xd4fc74,0xd4fc78..]
    [ 0xe92d49f8,0xe28db01c,0xe1a06000,0xe1a05001,0xe3a02000,0xe3a03001,0xebe33620
    , 0xe1a08000,0xe1a00006,0xe1a01005,0xe3a02000,0xe3a03003,0xebe3361a,0xe1a04000
    , 0xe1a00006,0xe1a01005,0xe3a02000,0xe3a03002,0xebe33614,0xe1a07000
    ]
  ++ zip [0x631a00,0x631a04..]
    [ 0xe92d09f0,0xe28db014,0xe300c286,0xe150000c,0x8a000028,0xe30bc09c,0xe340c081
    , 0xe79c8100,0xe3580000,0x0a000023,0xe1a0c008,0xe3a00000,0xe30f6fff,0xe1a0500c
    , 0xe59c4000,0xe1540006,0x1a000005,0xe59c7004,0xe1570006,0x1a000002,0xe59c7008
    , 0xe1570006,0x0a000016,0xe3520003,0x979ff102,0xea000010,0x00631a78,0x00631a84
    , 0x00631a90,0x00631aa0,0xe1510000,0x1a00000a,0xea000027,0xe1540001,0x1a000007
    , 0xea000024,0xe5954004,0xe1540001,0x1a000003,0xea000020,0xe5954008,0xe1540001
    , 0x0a00001d,0xe2800001,0xe28cc00c,0xeaffffde,0xe30e3dc8,0xe3403089,0xe3a02001
    , 0xe5832000,0xe3a00000,0xea000011,0xe0800080,0xe7980100,0xea000008,0xe0800080
    , 0xe0888100,0xe5980004,0xea000004,0xe0800080,0xe0888100,0xe5980008,0xea000000
    , 0xe3a00000,0xe370010a,0x030e3dc8,0x03403089,0x03a02001,0x05832000,0x03a00000
    , 0xe24bd014,0xe8bd09f0,0xe12fff1e,0xe30e2dc8,0xe3402089,0xe3a01000,0xe5821000
    , 0xe3530003,0x979ff103,0xeaffffee,0x00631b00,0x00631ad0,0x00631adc,0x00631aec
    ]

lifecycleReads :: LifecycleSite -> [(Word32,Int)]
lifecycleReads site = case site of
  CustomRefreshEntry -> custom
  CustomRefreshReturn -> custom
  CapturePreviousEntry -> images
  CapturePreviousReturn -> images
  RestorePreviousEntry -> images
  RestorePreviousReturn -> images
  ExchangeAlternateEntry -> images
  ExchangeAlternateReturn -> images
  EeCompare -> [(0x96cc64,0x1c2),(0x951564,4),(0x956864,4),(0x956868,4)]
  S1ActionEntry -> [(0x944580,0x50)]
  S1FlagReturn -> [(0x952004,4)]
  S1FrameCopyCall -> [(0x944580,0x50),(0x949dd8+0x5802,2)]
  S1DequeueEntry -> [(0x949dd8+0x5800,4)]
  S1DequeueCopyReturn -> [(0x944580,0x50)]
  S1ApplicationRouterCall -> [(0x944580,0x50),(0x1467588,2)]
  S1ApplicationDispatch -> [(0x944580,0x50),(0x146761c,2),(0x1467778,2)]
  _ -> []
  where
    images = [(0x96cc64,0x1c2),(0x96ce26,0x1c2),(0x89ec00,0x1c2),(0x9c281c,0x1c2)]
    custom = [(0x89f838,52),(0x89f874,52),(0x9c295c,40)]

exact :: [K.Key] -> Object -> Parser ()
exact names fields = unless (S.fromList (KM.keys fields) == S.fromList names) (fail "settings lifecycle fields differ")

closed :: Eq wire => String -> [(wire, value)] -> wire -> Parser value
closed label choices value = maybe (fail ("unknown " ++ label)) pure (lookup value choices)

name :: T.Text -> Parser MatrixStepName
name text = do
  unless (not (T.null text) && T.length text <= 64 && T.all (\c -> c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-') text) (fail "invalid lifecycle step name")
  pure (MatrixStepName text)

commit :: String -> Parser SourceCommit
commit text = do
  unless (length text == 40 && all isHexDigit text && all (\c -> not (c >= 'A' && c <= 'F')) text) (fail "invalid source commit")
  pure (SourceCommit text)

digest :: String -> Parser Sha256
digest = either fail pure . parseSha256Hex

instance FromJSON MatrixSource where
  parseJSON = withObject "lifecycle source" $ \fields -> do
    exact ["decomp_commit","image4_sha256","image5_sha256","catalog_commit","controls_commit"] fields
    MatrixSource <$> (fields .: "decomp_commit" >>= commit) <*> (fields .: "image4_sha256" >>= digest)
      <*> (fields .: "image5_sha256" >>= digest) <*> (fields .: "catalog_commit" >>= commit) <*> (fields .: "controls_commit" >>= commit)
instance ToJSON MatrixSource where
  toJSON source = object
    [ "decomp_commit" .= sourceCommitHex (matrixDecompCommit source), "image4_sha256" .= sha256Hex (matrixImage4 source)
    , "image5_sha256" .= sha256Hex (matrixImage5 source), "catalog_commit" .= sourceCommitHex (matrixCatalogCommit source)
    , "controls_commit" .= sourceCommitHex (matrixControlsCommit source) ]

instance FromJSON StorageAction where
  parseJSON = withObject "lifecycle action" $ \fields -> do
    kind <- fields .: "kind"
    case kind :: T.Text of
      "key" -> do
        exact ["kind","key"] fields
        fields .: "key" >>= closed "lifecycle key" ([(T.pack (crawlKeyWire key),PressKey key) | key <- [minBound..maxBound]] ++ [("half-press",PressHalfShutter)])
      "mode-dial" -> do
        exact ["kind","position"] fields
        SetModeDial <$> (fields .: "position" >>= closed "mode dial" [(T.pack (modeDialName position),position) | position <- [minBound..maxBound]])
      "cold-reboot" -> exact ["kind"] fields >> pure ColdReboot
      _ -> fail "unknown lifecycle action"
storageActionValue :: StorageAction -> Value
storageActionValue action = case action of
  PressKey key -> object ["kind" .= ("key"::T.Text),"key" .= crawlKeyWire key]
  PressHalfShutter -> object ["kind" .= ("key"::T.Text),"key" .= ("half-press"::T.Text)]
  SetModeDial position -> object ["kind" .= ("mode-dial"::T.Text),"position" .= modeDialName position]
  ColdReboot -> object ["kind" .= ("cold-reboot"::T.Text)]
instance ToJSON StorageAction where toJSON = storageActionValue

payloadNames :: [(T.Text,PayloadTarget)]
payloadNames = [("active-bank",ActiveCustomPayload),("backing",BackingCustomPayload),("previous-record",PreviousRecordPayload),("working-record",WorkingRecordPayload)]
payloadName :: PayloadTarget -> T.Text
payloadName target = case target of
  ActiveCustomPayload -> "active-bank"
  BackingCustomPayload -> "backing"
  PreviousRecordPayload -> "previous-record"
  WorkingRecordPayload -> "working-record"

instance FromJSON PayloadReference where
  parseJSON = withObject "payload reference" $ \fields -> do
    exact ["step","target"] fields
    PayloadReference <$> (fields .: "step" >>= name) <*> (fields .: "target" >>= closed "payload target" payloadNames)
instance ToJSON PayloadReference where
  toJSON reference = object ["step" .= matrixStepNameText (payloadReferenceStep reference),"target" .= payloadName (payloadReferenceTarget reference)]

instance FromJSON StorageCheck where
  parseJSON = withObject "lifecycle check" $ \fields -> do
    kind <- fields .: "kind"
    case kind :: T.Text of
      "custom-commit" -> do
        exact ["kind","change"] fields
        CustomCommit <$> (fields .: "change" >>= closed "record change" [("changed"::T.Text,Changed),("unchanged",Unchanged)])
      "bank-copy" -> do
        exact ["kind","family","direction"] fields
        BankCopy <$> (fields .: "family" >>= closed "copy family" [("previous"::T.Text,PreviousImage),("alternate",AlternateImage)])
          <*> (fields .: "direction" >>= closed "copy direction" [("capture"::T.Text,Capture),("restore",Restore)])
      "custom-payload" -> do
        exact ["kind","relation","target","reference"] fields
        CustomPayload <$> (fields .: "relation" >>= closed "payload relation" [("same"::T.Text,SameAs),("different",DifferentFrom)])
          <*> (fields .: "target" >>= closed "payload target" payloadNames) <*> fields .: "reference"
      "current-slot" -> do
        exact ["kind","slot"] fields
        CurrentSlot <$> (fields .: "slot" >>= closed "profile slot" [("custom"::T.Text,StoredCustom),("live",LiveProfile)])
      "ui-handback" -> exact ["kind"] fields >> pure UiHandback
      "repeat-setting" -> do
        exact ["kind","setting"] fields
        RepeatSetting <$> (fields .: "setting" >>= closed "storage setting" [("film-simulation"::T.Text,FilmSimulation),("white-balance-shift",WhiteBalanceShift)])
      "cold-boot-record" -> do
        exact ["kind","reference"] fields
        ColdBootRecord <$> (fields .: "reference" >>= name)
      _ -> fail "unknown lifecycle check"
storageCheckValue :: StorageCheck -> Value
storageCheckValue check = case check of
  CustomCommit change -> object ["kind" .= ("custom-commit"::T.Text),"change" .= case change of Changed -> ("changed"::T.Text); Unchanged -> "unchanged"]
  BankCopy family direction -> object ["kind" .= ("bank-copy"::T.Text),"family" .= case family of PreviousImage -> ("previous"::T.Text); AlternateImage -> "alternate","direction" .= case direction of Capture -> ("capture"::T.Text); Restore -> "restore"]
  CustomPayload relation target reference -> object ["kind" .= ("custom-payload"::T.Text),"relation" .= case relation of SameAs -> ("same"::T.Text); DifferentFrom -> "different","target" .= payloadName target,"reference" .= reference]
  CurrentSlot slot -> object ["kind" .= ("current-slot"::T.Text),"slot" .= case slot of StoredCustom -> ("custom"::T.Text); LiveProfile -> "live"]
  UiHandback -> object ["kind" .= ("ui-handback"::T.Text)]
  RepeatSetting setting -> object ["kind" .= ("repeat-setting"::T.Text),"setting" .= case setting of FilmSimulation -> ("film-simulation"::T.Text); WhiteBalanceShift -> "white-balance-shift"]
  ColdBootRecord reference -> object ["kind" .= ("cold-boot-record"::T.Text),"reference" .= matrixStepNameText reference]
instance ToJSON StorageCheck where toJSON = storageCheckValue

instance FromJSON StorageStep where
  parseJSON = withObject "lifecycle step" $ \fields -> do
    exact ["name","action","checks"] fields
    StorageStep <$> (fields .: "name" >>= name) <*> fields .: "action" <*> fields .: "checks"
instance ToJSON StorageStep where
  toJSON step = object ["name" .= matrixStepNameText (storageStepName step),"action" .= storageStepAction step,"checks" .= storageStepChecks step]

instance FromJSON StorageMatrix where
  parseJSON = withObject "settings lifecycle matrix" $ \fields -> do
    exact ["schema","kind","source","steps"] fields
    schema <- fields .: "schema"
    kind <- fields .: "kind"
    unless (schema == (1::Int) && kind == ("settings-storage-lifecycle"::T.Text)) (fail "settings lifecycle schema differs")
    source <- fields .: "source"
    steps <- fields .: "steps"
    unless (not (null steps)) (fail "settings lifecycle matrix is empty")
    let names = map storageStepName steps
    unless (S.size (S.fromList names) == length names) (fail "lifecycle step names repeat")
    forM_ (zip [0 :: Int ..] steps) $ \(index,step) -> forM_ (storageStepChecks step) $ \check -> do
      let references = case check of CustomPayload _ _ reference -> [payloadReferenceStep reference]; ColdBootRecord reference -> [reference]; _ -> []
      unless (all (`elem` take index names) references) (fail "lifecycle check refers to a nonpreceding step")
      case check of ColdBootRecord _ -> unless (storageStepAction step == ColdReboot) (fail "cold-boot check needs a cold-reboot action"); _ -> pure ()
    pure (StorageMatrix source steps)
storageMatrixValue :: StorageMatrix -> Value
storageMatrixValue matrix = object ["schema" .= (1::Int),"kind" .= ("settings-storage-lifecycle"::T.Text),"source" .= matrixSource matrix,"steps" .= matrixSteps matrix]
instance ToJSON StorageMatrix where toJSON = storageMatrixValue

decodeStorageMatrix :: B.ByteString -> Either String StorageMatrix
decodeStorageMatrix = eitherDecodeStrict'
