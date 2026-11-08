{-# LANGUAGE OverloadedStrings #-}
module Emulation.SchedulerPairCatalog where

import Control.Monad (forM, unless)
import Data.Aeson (FromJSON (..), Value, object, withObject, (.:), (.:?), (.=))
import qualified Data.Aeson as J
import Data.Aeson.Types (Parser)
import qualified Data.ByteString as B
import Data.List (find)
import qualified Data.Text as T
import Data.Word (Word32)
import Numeric (readHex, showHex)
import Replacement.RtosOwnFirst.Regions (Domain (..), domains)
import qualified Replacement.RtosScheduler.Regions as R

newtype Address = Address Word32 deriving (Eq,Ord,Show)
hex :: Word32 -> String
hex word = "0x" ++ showHex word ""
parseHex :: Value -> Parser Word32
parseHex = J.withText "address" $ \text -> case T.unpack text of
  '0':'x':digits | [(word, "")] <- readHex digits, word <= toInteger (maxBound :: Word32) -> pure (fromInteger word)
  _ -> fail "invalid word"
parseDomain :: Value -> Parser Domain
parseDomain = J.withText "domain" $ \text -> case text of "a" -> pure DomainA; "b" -> pure DomainB; _ -> fail "invalid domain"

data DispatchPoint = Save | Resume | NoSave | Select | FirstEntry | IrqReturn | IrqResume | IrqTail deriving (Eq,Ord,Show,Enum,Bounded)
data Service = DisableDispatch | EnableDispatch | ExitTask | RotateReady | InterruptRotateReady | CoreRotateReady | InterruptCoreRotateReady
  | InterruptActivateTask | ActivateTask | CancelActivation | WakeTask | InterruptWakeTask | CancelWakeup
  | GetTaskPriority | SleepTask | SuspendTask | ResumeTask | ForceResumeTask | RaiseTaskException | InterruptRaiseTaskException
  | DisableTaskException | EnableTaskException | DeliverTaskException | DeliverTaskExceptionEntry deriving (Eq,Ord,Show,Enum,Bounded)
data FamilyService = FamilyRelease | HelperRelease | HelperBlock | HelperBlockTimed | HelperCheck
  | FlagCreate | FlagDelete | FlagSet | FlagSetHandler | FlagClear | FlagWait | FlagPoll | FlagTimedWait
  | MailboxCreate | MailboxDelete | MailboxSend | MailboxReceive | MailboxPoll | MailboxTimedReceive deriving (Eq,Ord,Show,Enum,Bounded)
familyServiceName :: FamilyService -> String
familyServiceName service = ["family-release","helper-release","helper-block","helper-block-timed","helper-check",
  "family-flag-create","family-flag-delete","family-flag-set","family-flag-set-handler","family-flag-clear",
  "family-flag-wait","family-flag-poll","family-flag-timed-wait","family-mailbox-create","family-mailbox-delete",
  "family-mailbox-send","family-mailbox-receive","family-mailbox-poll","family-mailbox-timed-receive"] !! fromEnum service
data SemaphoreService = SemaphoreSignal | SemaphoreInterruptSignal | SemaphoreWait | SemaphorePoll | SemaphoreTimedWait | SemaphoreCreate | SemaphoreDelete deriving (Eq,Ord,Show,Enum,Bounded)
semaphoreServiceName :: SemaphoreService -> String
semaphoreServiceName service = ["signal","interrupt-signal","wait","poll","timed-wait","create","delete"] !! fromEnum service
data Semantic = HelperSemantic R.Helper | DispatchSemantic DispatchPoint | ServiceSemantic Service | FamilySemantic FamilyService | SemaphoreSemantic SemaphoreService | DoorbellSemantic deriving (Eq,Ord,Show)
serviceName :: Service -> String
serviceName service = ["disable-dispatch","enable-dispatch","exit-task","rotate-ready","interrupt-rotate-ready","core-rotate-ready","interrupt-core-rotate-ready",
  "interrupt-activate-task","activate-task","cancel-activation","wake-task","interrupt-wake-task","cancel-wakeup",
  "get-task-priority","sleep-task","suspend-task","resume-task","force-resume-task","raise-task-exception","interrupt-raise-task-exception",
  "disable-task-exception","enable-task-exception","deliver-task-exception","deliver-task-exception-entry"] !! fromEnum service
dispatchName :: DispatchPoint -> String
dispatchName point = ["save","resume","no-save","select","first-entry","irq-return","irq-resume","irq-tail"] !! fromEnum point
named :: (Eq a, Enum a, Bounded a) => (a -> String) -> String -> Parser a
named name text = maybe (fail ("unknown semantic name " ++ text)) pure (find ((==text) . name) [minBound..maxBound])
semanticValue :: Semantic -> Value
semanticValue semantic = object ["kind" .= (kind::String),"name" .= name]
 where
  (kind,name) = case semantic of
    HelperSemantic helper -> ("helper",R.helperName helper)
    DispatchSemantic point -> ("dispatch",dispatchName point)
    ServiceSemantic service -> ("service",serviceName service)
    FamilySemantic service -> ("family",familyServiceName service)
    SemaphoreSemantic service -> ("semaphore",semaphoreServiceName service)
    DoorbellSemantic -> ("irq","doorbell")
instance FromJSON Semantic where
  parseJSON = withObject "entry semantic" $ \o -> do
    kind <- o .: "kind"
    name <- o .: "name"
    case (kind::String) of
      "helper" -> HelperSemantic <$> named R.helperName name
      "dispatch" -> DispatchSemantic <$> named dispatchName name
      "service" -> ServiceSemantic <$> named serviceName name
      "family" -> FamilySemantic <$> named familyServiceName name
      "semaphore" -> SemaphoreSemantic <$> named semaphoreServiceName name
      "irq" | name=="doorbell" -> pure DoorbellSemantic
      _ -> fail "unknown semantic kind"

data ProbeKind = SelectedRead | DiscardedRead | EnabledRead | GlobalLockRead | Retry | Callback | CallbackReturn | SelectComplete | FirstEntryExit | IrqTailProbe | TimerInsert | TimerRemove | TimerConsume deriving (Eq,Ord,Show,Enum,Bounded)
newtype Register = Register Int deriving (Eq,Ord,Show)
data Probe = Probe { probeDomain :: Domain, probeAddress :: Address, probeKind :: ProbeKind, probeRegister :: Register } deriving (Eq,Show)
probeKindName :: ProbeKind -> String
probeKindName SelectedRead = "selected-read"
probeKindName DiscardedRead = "discarded-read"
probeKindName EnabledRead = "enabled-read"
probeKindName GlobalLockRead = "global-lock-read"
probeKindName Retry = "retry"
probeKindName Callback = "callback"
probeKindName CallbackReturn = "callback-return"
probeKindName SelectComplete = "select-complete"
probeKindName FirstEntryExit = "first-entry-exit"
probeKindName IrqTailProbe = "irq-tail"
probeKindName TimerInsert = "timer-insert"
probeKindName TimerRemove = "timer-remove"
probeKindName TimerConsume = "timer-consume"
registerName :: Register -> String
registerName (Register index) = "r" ++ show index
registerOf :: String -> Parser Register
registerOf text = case [(index,rest) | 'r':digits <- [text],(index,rest) <- reads digits] of
  [(index,"")] | index>=0 && index<16 -> pure (Register index)
  _ -> fail "register must be r0..r15"

data Entry = Entry { entryDomain :: Domain, entrySemantic :: Semantic, entryAddress :: Address } deriving (Eq,Show)
data Window = Window { windowDomain :: Domain, windowStart :: Address, windowEnd :: Address } deriving (Eq,Ord,Show)
data Catalog = Catalog { catalogFill :: Word32, catalogEntries :: [Entry], catalogWindows :: [Window], catalogProbes :: [Probe] } deriving (Eq,Show)

instance FromJSON Catalog where
  parseJSON = withObject "scheduler regions" $ \o -> do
    fill <- o .: "fill_word" >>= parseHex
    tables <- o .: "domains"
    rows <- forM domains $ \domain -> do
      table <- tables .: (if domain==DomainA then "a" else "b")
      regionRows <- table .: "regions"
      parsed <- forM regionRows $ withObject "owned extent" $ \region -> do
        lo <- Address <$> (region .: "start" >>= parseHex)
        hi <- Address <$> (region .: "end" >>= parseHex)
        nested <- region .:? "entries"
        entries <- case nested of
          Just values -> mapM (entryOf domain) values
          Nothing -> do
            address <- Address <$> (region .: "entry" >>= parseHex)
            semantic <- region .:? "semantic" >>= maybe (HelperSemantic <$> (region .: "helper" >>= named R.helperName)) pure
            pure [Entry domain semantic address]
        unless (lo<hi && all (\entry -> lo<=entryAddress entry && entryAddress entry<hi) entries) (fail "entry outside its extent")
        pure (entries,Window domain lo hi)
      shims <- table .:? "shims"
      bodies <- forM (maybe [] id shims) $ withObject "shim" $ \shim -> do
        body <- shim .: "body"
        Window domain <$> (Address <$> (body .: "address" >>= parseHex)) <*> (Address <$> (body .: "end" >>= parseHex))
      rawProbes <- table .:? "probes"
      probes <- mapM (probeOf domain) (maybe [] id rawProbes)
      pure (concatMap fst parsed,map snd parsed ++ bodies,probes)
    pure (Catalog fill (concat [entries | (entries,_,_) <- rows]) (concat [windows | (_,windows,_) <- rows]) (concat [probes | (_,_,probes) <- rows]))
   where
    entryOf domain = withObject "entry" $ \entry -> Entry domain <$> entry .: "semantic" <*> (Address <$> (entry .: "entry" >>= parseHex))
    probeOf domain = withObject "probe" $ \probe -> Probe domain <$> (Address <$> (probe .: "address" >>= parseHex))
      <*> (probe .: "kind" >>= named probeKindName) <*> (probe .: "register" >>= registerOf)
parseCatalog :: B.ByteString -> Either String Catalog
parseCatalog = J.eitherDecodeStrict
