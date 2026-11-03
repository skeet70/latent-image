{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE GHC2021 #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import qualified A32 as A
import Control.Monad (unless)
import qualified Crypto.Hash.SHA256 as SHA256
import Data.Aeson (FromJSON (parseJSON), ToJSON, Value, encode, object, (.=))
import Data.Aeson.Types (parseEither)
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as L
import Data.Bits (shiftL, (.|.))
import Data.Char (isHexDigit)
import Data.List (isInfixOf, nub, sort, stripPrefix)
import qualified Data.Map.Strict as M
import Data.Maybe (mapMaybe)
import Data.Word (Word32)
import qualified FujiAF.JSON as JSON
import GHC.Generics (Generic)
import Numeric (readHex, showHex)
import StockInventory (FunctionDeclaration (..), FunctionDerivation (..), FunctionSize (..), FunctionWindow (..), GuestAddress (..), deriveFunction, returnPremiseLabel)
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype SlotOffset = SlotOffset Word32 deriving (Eq, Ord, Show)
newtype EntryAddress = EntryAddress Word32 deriving (Eq, Ord, Show)
data Domain = A | B deriving (Eq, Show)
data Recovery = Recovered | Partial | Unknown deriving (Show)
data Contract = Contract
  { operation :: String
  -- | A µITRON 4.0 service whose behavior the X-M1 body matches, with stated differences.
  , analogue :: Maybe String
  , arguments :: Maybe String
  , result :: Maybe String
  , errors :: [(Int, String)]
  , context :: Maybe String
  , objects :: [String]
  , computedWrappers :: [Word32]
  , evidence :: [String]
  , recovery :: Recovery
  } deriving (Show)
data InputIdentity = InputIdentity { inputPath :: FilePath, inputBytes :: Int, inputSha256 :: String } deriving (Generic)
instance ToJSON InputIdentity

digest :: B.ByteString -> String
digest = concatMap (\byte -> let digits = showHex byte "" in replicate (2-length digits) '0' ++ digits) . B.unpack . SHA256.hash

data Surface = Surface
  { surfaceName :: String
  , entries :: [String]
  , surfaceContract :: String
  , surfaceEvidence :: [String]
  } deriving (Generic)
instance FromJSON Surface
instance ToJSON Surface

data DeviceLaw = DeviceLaw
  { lawName :: String
  , grounded :: String
  , missing :: String
  , scenarios :: [String]
  , lawEvidence :: [String]
  } deriving (Generic)
instance FromJSON DeviceLaw
instance ToJSON DeviceLaw

data SurfaceMap = SurfaceMap
  { nonRoot :: [Surface]
  , deviceLaws :: [DeviceLaw]
  , objectConsumers :: [Surface]
  , remainingClosure :: [String]
  } deriving (Generic)
instance FromJSON SurfaceMap
instance ToJSON SurfaceMap

newtype Sha256 = Sha256 String deriving (Eq)
instance FromJSON Sha256 where
  parseJSON value = parseJSON value >>= \text ->
    if length text == 64 && all (`elem` ("0123456789abcdef" :: String)) text then pure (Sha256 text) else fail ("not a lowercase SHA256: " ++ text)

instance FromJSON Domain where
  parseJSON value = parseJSON value >>= \name -> case (name :: String) of
    "A" -> pure A
    "B" -> pure B
    _ -> fail ("unknown domain " ++ name)

newtype FunctionStart = FunctionStart Word32 deriving (Eq)
instance FromJSON FunctionStart where
  parseJSON value = parseJSON value >>= \text -> case stripPrefix "0x" text of
    Just digits | not (null digits) && length digits <= 8 && all isHexDigit digits, [(number, "")] <- readHex digits -> pure (FunctionStart number)
    _ -> fail ("function start is not 0x-prefixed 32-bit hexadecimal: " ++ text)

-- A reviewed start of an unpublished function, bound to one domain's loader image.
data FunctionBoundary = FunctionBoundary
  { boundaryDomain :: Domain
  , loaderSha256 :: Sha256
  , boundaryStart :: FunctionStart
  , boundaryEvidence :: [String]
  , basis :: String
  } deriving (Generic)
instance FromJSON FunctionBoundary

newtype FunctionBoundaries = FunctionBoundaries { boundaries :: [FunctionBoundary] } deriving (Generic)
instance FromJSON FunctionBoundaries

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

publisherEntries :: String -> Either String [(SlotOffset, EntryAddress)]
publisherEntries source = do
  let serviceLines = takeWhile (not . isInfixOf "];") (drop 1 (dropWhile (not . isInfixOf "const SERVICES:") (lines source)))
      services = concatMap hexValues serviceLines
      extensionLines = [line | line <- lines source, any (`isInfixOf` line) ["(0x1618)", "(0x161c)", "(0x1620)", "(0x1624)"]]
      extensions = [ (SlotOffset offset, EntryAddress target) | line <- extensionLines, [offset, target] <- [hexValues line] ]
  unless (length services == 93 && length extensions == 4) (Left "publisher must contain 93 service entries and four extensions")
  pure (zipWith (\index address -> (SlotOffset (4*index), EntryAddress address)) [0..] services ++ extensions)

hexValues :: String -> [Word32]
hexValues [] = []
hexValues ('0':'x':rest) = case readHex (filter (/= '_') digits) of
  [(number, "")] -> number : hexValues suffix
  _ -> []
  where (digits, suffix) = span (\c -> isHexDigit c || c == '_') rest
hexValues (_:rest) = hexValues rest

wordAt :: B.ByteString -> Int -> Word32
wordAt image offset = foldr (\index value -> (value `shiftL` 8) .|. fromIntegral (B.index image (offset+index))) 0 [0..3]

instructions :: Word32 -> B.ByteString -> [(Word32, A.Instruction)]
instructions base image = [(base+fromIntegral offset, A.decode (base+fromIntegral offset) (wordAt image offset)) | offset <- [0,4..B.length image-4]]

functionStarts :: String -> [Word32]
functionStarts listing = mapMaybe start (lines listing)
  where
    start line = do
      rest <- stripPrefix "// ===== " line
      case readHex (takeWhile isHexDigit rest) of
        [(number, "")] -> Just number
        _ -> Nothing

extent :: Word32 -> B.ByteString -> [Word32] -> EntryAddress -> Value
extent base image starts (EntryAddress address) =
  let imageEnd = base + fromIntegral (B.length image)
      fence = minimum (imageEnd : [entry | entry <- starts, entry > address])
      outcome = deriveFunction (GuestAddress base) image (FunctionWindow (GuestAddress address) (GuestAddress fence)) []
  in case outcome of
    Right derived -> case derivedDeclaration derived of
      FunctionDeclaration _ (FunctionSize bytes) -> object
        [ "status" .= ("derived" :: String), "start" .= hex address, "end" .= hex (address+fromIntegral bytes)
        , "codeEnvelopeBytes" .= bytes, "windowEnd" .= hex fence
        , "returnPremises" .= map returnPremiseLabel (derivationPremises derived)
        , "scope" .= ("StockInventory reachable A32 code envelope, not transitive code size; complete candidate window classified as code or literals" :: String)
        ]
    Left problem -> object
      [ "status" .= ("unknown" :: String), "start" .= hex address, "windowEnd" .= hex fence
      , "refusal" .= problem, "scope" .= ("No extent claimed; next entry candidate is a fence, not a measured function end" :: String)
      ]

contract :: SlotOffset -> Contract
contract (SlotOffset offset) = M.findWithDefault unknown offset known
  where
    unknown = Contract "unknown" Nothing Nothing Nothing [] Nothing [] []
      ["replacement/primary_initializer/src.rs:25-49,93-96", "replacement/interface_initializer/src.rs:25-49,78-81"] Unknown
    c name args returned failures required touched wrappers citations = Contract name Nothing (Just args) (Just returned) failures (Just required) touched wrappers citations Partial
    partialAs like name args returned failures required touched wrappers citations = Contract name like (Just args) (Just returned) failures (Just required) touched wrappers (citations ++ censusEvidence) Partial
    recoveredAs like name args returned failures required touched wrappers citations = Contract name like (Just args) (Just returned) failures (Just required) touched wrappers (citations ++ censusEvidence) Recovered
    censusEvidence = ["E-RTOS-SLOT-CENSUS-STOCK-150", "private-data/scratch/RtosCensus/census-v1/census.json .reach[]"]
    dis domain range = "private-data/analysis/x-m1-1.50/decode/" ++ (if domain == A then "loader-40000.dis:" else "loader-d0000.dis:") ++ range
    contracts family = "private-data/scratch/RtosCensus/contracts/" ++ family ++ ".json"
    classSelect = "class r0[31:16] (0 executing domain via A 0x400545c8 / B 0x400e4588, 1 domain A, 2 domain B records at A 0x400545cc / B 0x400e458c + (class-1)*132, else -18); n = sxth(r0)"
    taskCheck = "Requires context word (A 0x400545a4 / B 0x400e4564) = 0 and CPSR.I clear at entry, else -25; masks IRQ and spins on the selected class record's lock"
    taskEvidence = ["reports/replacement-inventory-baseline.md:1074-1130", "E-STARTUP-FIRST-RUNNABLE-SOURCE-150", "E-STARTUP-TASK-STOCK-QUALIFIED-150"]
    queueEvidence = ["reports/replacement-inventory-baseline.md:961-969", "emulation/README.md:498-514", "E-XM1-QUEUE-CALLER-BOUNDARY-150"]
    standbyEvidence = ["reports/replacement-inventory-baseline.md:1573-1593", "E-INITIAL-TASK-BODIES-SOURCE-150"]
    criticalEvidence = ["reports/replacement-inventory-baseline.md:614-624", "reports/replacement-inventory-baseline.md:1100-1102", "E-STARTUP-TASK-WRAPPER-SOURCE-150"]
    known = M.fromList
      [ (0x0, c "task-record-create" "r0 encoded class/id (low16 selects record), r1 pointer to {flags,arg,entry,priority,bytes,base}" "r0 status; success 0" [(-41,"C+36 already nonzero")] "Masks/restores IRQ and FIQ internally; startup called with IRQ masked; general invalid-input contract incomplete" ["C[120]", "T[120]", "creation-id-map", "CPU mask"] [0x725dd8] taskEvidence)
      , (0xc, c "task-activate" "r0 encoded class/id; id0 selects current task" "r0 status; ready/pending transition on success" [(-25,"nonzero context or IRQ masked"),(-18,"invalid signed class/id/domain selection"),(-42,"creation record absent"),(-43,"already pending activation")] "Task context and unmasked IRQ required; selected-root/current-core distinction applies" ["C[120]", "T[120]", "scheduler domains", "ready queues", "priority bitmap", "cross-core notification"] [0x725dd8] taskEvidence)
      , (0x4c, c "task-delay" "r0 ticks, added without tenfold scaling" "Status domain incomplete" [] "Current-task context; complete refusal rules unrecovered" ["current T", "wake deadline queue", "clock pair"] [0x723ce8] ["E-AUTOACT-UNIT-150", "reports/auto-act-arming.md section 13.2"])
      , (0x64, c "resource-create" "r0 id; r1 parameters; wrapper constructs {0,1,1}" "r0 status; root104 ignores result" [] "Current-core root selected; full context rules unknown" ["resource/semaphore objects"] [0x7247ac] ["reports/replacement-inventory-baseline.md:1602", "E-INITIAL-TASK-BODIES-SOURCE-150"])
      , (0x6c, c "resource-give-task" "Wrapper resource id; lower ABI incomplete" "r0 status; complete errors unknown" [] "Task form, full admission unknown" ["resource/semaphore objects", "waiters"] [] ["reports/replacement-inventory-baseline.md:1398-1408"])
      , (0x70, c "resource-give-contextual" "Wrapper resource id; lower ABI incomplete" "r0 status; complete errors unknown" [] "Alternate-context form, precise IRQ constraints unknown" ["resource/semaphore objects", "waiters"] [] ["reports/replacement-inventory-baseline.md:1398-1408"])
      , (0x7c, c "resource-take" "Wrapper resource id and timeout; lower ABI incomplete" "r0 status; complete errors unknown" [] "Potentially blocking; complete task/IRQ rules unknown" ["resource/semaphore objects", "waiters", "scheduler"] [] ["reports/replacement-inventory-baseline.md:1398-1408"])
      , (0x80, c "task-auxiliary-register" "r0 encoded class/id (low16 selects record), r1 pointer to {flags,0}; wrapper flags 0 or4" "r0 status; success0" [(-41,"auxiliary record+8 already nonzero")] "Preserves prior IRQ/FIQ mask; existing node pointers required" ["auxiliary[120]", "auxiliary-list-node[120]", "CPU mask"] [0x725dd8] taskEvidence)
      , (0x88, c "flag-set-task" "Wrapper destination task/class and bitmask; exact lower register tuple incomplete" "r0 status; OR bits and ready satisfied waiter" [] "Task-context variant" ["task-associated flags", "waiter", "ready queue"] [0x724558] standbyEvidence)
      , (0x8c, c "flag-set-interrupt" "Wrapper destination task/class and bitmask; exact lower register tuple incomplete" "r0 status; OR bits and ready satisfied waiter" [] "Interrupt-context variant; exact masks unknown" ["task-associated flags", "waiter", "ready queue"] [0x724558] standbyEvidence)
      , (0x9c, c "flag-wait" "Encoded event id, mask, mode, output pointer, timeout; exact register/stack placement not recovered here; B93 wrapper (1,&out,-1), encoded id0x2005d" "Satisfied mode0 writes whole flag word; attribute4 clears whole flag word" [(-50,"wrapper treats alongside success without reporting")] "Rejects invalid task/IRQ context; mode/mask/id/timeout/object/existing-waiter checks; numerical errors incomplete" ["task-associated flags", "wait request/result", "T", "scheduler"] [0x724480] standbyEvidence)
      , (0xa0, c "mailbox-create" "Wrapper(id,{count,size}); exact lower parameters incomplete" "status; cleanup on failure does not undo every publication" [] "Current-core selected root; full admission unknown" ["mailbox", "handle arrays", "size arrays"] [0x724cdc] queueEvidence)
      , (0xa4, c "mailbox-construction-cleanup" "Lower tuple incomplete" "status; rollback scope incomplete" [] "Construction failure path" ["mailbox", "handle arrays"] [0x724cdc] queueEvidence)
      , (0xac, c "message-submit-task" "Message buffer from pool, encoded receiver; exact lower tuple incomplete" "0 success; finite queue full may return -50" [(-50,"finite queue full")] "Task variant; full task/IRQ/mask admission unknown" ["mailbox", "message buffer", "waiters"] [0x72503c] queueEvidence)
      , (0xb0, c "message-submit-contextual" "Message buffer from pool, encoded receiver; exact lower tuple incomplete" "0 success; finite queue full may return -50" [(-50,"finite queue full")] "Context-selected alternate variant; precise IRQ rules unknown" ["mailbox", "message buffer", "waiters"] [0x72503c] queueEvidence)
      , (0xc8, c "message-receive" "Wrapper(queue,destination,timeout), lower writes returned buffer pointer" "0 success; finite empty queue -50; wrapper copies before release" [(-50,"finite empty queue")] "Potentially blocks; full context checks unknown" ["mailbox", "pool message", "destination", "waiters"] [0x725318] queueEvidence)
      , (0xe4, c "message-pool-create" "Constructor-derived pool parameters; complete tuple unknown" "r0 status" [] "Constructor path; full context unknown" ["message pool", "handle arrays", "size arrays"] [0x724cdc] queueEvidence)
      , (0xf8, c "message-buffer-release" "Received/acquired pool buffer; complete tuple unknown" "r0 status ignored by receive wrapper; masked send cleanup can refuse before free" [] "Mask-sensitive refusal; complete checks unknown" ["message pool", "buffer ownership"] [0x72503c,0x725318] queueEvidence)
      , (0x148, recoveredAs (Just "sns_ctx") "non-task-context-sense" "None read" "r0 = 1 when the context word (A 0x400545a4 / B 0x400e4564) is nonzero, else 0; the word counts IRQ nesting (vector increments, exit decrements) and the dispatcher idle loop sets it to 1" [] "Any context; reads one word, no mask or lock" ["context word"] [0x72503c] (queueEvidence ++ [dis A "13020-13029", dis B "13004-13013", dis A "142-145", dis A "217-222"]))
      , (0x14c, recoveredAs (Just "sns_loc") "irq-mask-sense" "None read" "r0 = CPSR bit 7 (1 when IRQ is masked)" [] "Any context; no state access; not reached in the stock census" ["CPSR.I"] [] [dis A "13030-13036", dis B "13014-13020"])
      , (0x150, recoveredAs (Just "sns_dsp") "dispatch-disabled-sense" "None read" "r0 = 1 when the dispatch-enabled word (A 0x40054764 / B 0x400e4724) is 0, else 0" [] "Any context; reads one word; not reached in the stock census" ["dispatch-enabled word"] [] [dis A "13037-13046", dis B "13021-13030"])
      , (0x154, partialAs (Just "consistent with sns_dpn") "dispatch-pending-state-sense" "None read" "r0 = 1 when the context word is nonzero, CPSR.I is set or the dispatch-enabled word is 0, else 0" [] "Any context; two RAM reads and a CPSR sample without a lock; not reached in the stock census" ["context word", "dispatch-enabled word", "CPSR.I"] [] [dis A "13047-13066", dis B "13031-13050"])
      , (0x4, recoveredAs Nothing "task-record-delete" "r0: only uxth(r0) is used, index id-1 into the executing domain's C/T tables, class bits ignored, no range check (id 0 writes C[-1]); r1 mode: 1 also rewrites C to defaults {flags 0, arg id-1, entry 0, priority 15, stack bytes 4, stack base A 0x40062c50 / B 0x400f2c10, id, created 0}, the creation-id map entry and resets T dormant (T+0x20 domain tag A 1 / B 2); other r1 values clear only C+0x24" "r0 = 0 on every path" [] "No context, existence, state or lock check; masks IRQ and FIQ around the body; stock callers run it on the current task before +0x14 or on another task after deactivation" ["C[id-1] A 0x4005beac / B 0x400ebe6c stride 40", "T[id-1] A 0x4005ff0c / B 0x400efecc stride 56", "creation-id map A 0x400622d4 / B 0x400f2294", "CPSR I/F"] [0x72612c,0x7289b0,0x728bac] [dis A "13391-13455", dis B "13376-13440", contracts "task-family"])
      , (0x14, recoveredAs (Just "ext_tsk") "task-exit" "None read" "Does not return: unlinks the current T from its ready queue, resets it dormant (restarting it if T+0xe bit0 queued an activation), sets the dispatch-enabled word to 1 and enters the non-saving dispatcher A 0x43688 / B 0xd3688" [] "Current task of the executing core; takes the executing domain's scheduler lock; no context-word check; every task entry returns here through the start trampoline A 0x43728 / B 0xd3728" ["current T", "C", "ready queues", "priority bitmap", "dispatch-enabled word", "context word"] [0x72612c,0x728b7c,0x728bac] [dis A "5346-5409", dis B "5330-5393", contracts "task-family"])
      , (0x1c, recoveredAs (Just "chg_pri") "task-priority-change" (classSelect ++ "; n 0 selects the executing core's current T, n > record+0x48 or n < 0 is -18; r1 external priority 0..16, 0 restores the creation priority C+0xc") "r0 = 0; a ready task is requeued at the tail of its new priority and may dispatch locally or notify the peer core through 0x433e4(record+0 >> 1, 120); a task waiting in a priority-ordered object queue is reordered" [(-25,"context word nonzero or CPSR.I set"),(-18,"invalid class or id"),(-17,"r1 > 16 unsigned"),(-42,"C+0x24 created flag clear"),(-41,"T dormant, or T is the executing core's current task with T+0x30 set")] taskCheck ["class record", "T +0xc/+0xd/+0x14/+0x28/+0x30/+0x34", "C +0xc/+0x24", "ready queues", "object wait queue"] [0x723a14,0x72905c] [dis A "5617-5813", dis B "5601-5797", contracts "task-family"])
      , (0x20, recoveredAs (Just "get_pri") "task-priority-get" (classSelect ++ "; n 0 selects the executing core's current T; r1 output word pointer, not validated") "r0 = 0 with *r1 = T+0xd + 1 (external 1..16); *r1 untouched on error" [(-25,"context word nonzero or CPSR.I set"),(-18,"invalid class or id"),(-42,"C+0x24 created flag clear"),(-41,"T dormant, or current task with T+0x30 set")] taskCheck ["class record", "T +0xc/+0xd/+0x30", "C +0x24", "*r1"] [0x723acc,0x728ea0,0x728f94] [dis A "5814-5914", dis B "5798-5898", contracts "task-family"])
      , (0x24, recoveredAs (Just "slp_tsk") "task-sleep" "None read" "r0 = 0 when a queued wakeup (T+0xe bit1) is consumed without blocking; otherwise the task sleeps (T+0xc = 10) on its result slot A 0x4005477c / B 0x400e473c + (id-1)*4 and returns 0 when woken through 0x44568 or -49 when released through 0x44744; a task terminated while sleeping does not return" [(-25,"context word nonzero, CPSR.I set, or dispatch-enabled word 0"),(-49,"wait released through 0x44744")] "Current task only; task context, IRQ unmasked and dispatch enabled; dispatches with context save" ["current T", "result slots", "ready queues", "dispatch-enabled word"] [0x7240c8,0x728c60] [dis A "5915-6010", dis B "5899-5994", contracts "task-family"])
      , (0x2c, recoveredAs (Just "wup_tsk") "task-wakeup" (classSelect ++ "; n 0 selects the executing core's current T; r1 not read") "r0 = 0; a sleeping target is released through 0x44568 (result slot 0, timer cancelled, readied, local dispatch or peer notification), otherwise one wakeup is queued in T+0xe bit1" [(-25,"context word nonzero or CPSR.I set"),(-18,"invalid class or id"),(-42,"C+0x24 created flag clear"),(-41,"T dormant, or current task with T+0x30 set"),(-43,"target not sleeping and a wakeup already queued")] taskCheck ["class record", "T +0xc/+0xe/+0x14/+0x24", "waiter result slot", "ready queues"] [0x724144,0x728d24] [dis A "6119-6245", dis B "6103-6229", contracts "task-family"])
      , (0x30, recoveredAs (Just "iwup_tsk") "task-wakeup-from-handler" (classSelect ++ "; n must be 1..record+0x48, no self selector; r1 not read") "r0 = 0; as +0x2c, but a needed local dispatch sets the delayed-dispatch word A 0x400546d8 / B 0x400e4698 instead of dispatching" [(-25,"context word 0 or CPSR.I set"),(-18,"invalid class or id"),(-42,"C+0x24 created flag clear"),(-41,"T dormant"),(-43,"target not sleeping and a wakeup already queued")] "Requires a nonzero context word and CPSR.I clear; spins on the class record lock with IRQ masked; never dispatches directly" ["class record", "T", "waiter result slot", "delayed-dispatch word"] [0x724144,0x728d24] [dis A "6246-6347", dis B "6230-6331", contracts "task-family"])
      , (0x68, recoveredAs Nothing "semaphore-delete" "r0: only uxth(r0) is used, index n-1 into the executing domain's 250-entry tables, class bits ignored, no range check" "r0 = 0 on every path; record reset to {attr 0, initial 1, max 1, exists 0} and current count set to 1; queued waiters are neither released nor unlinked" [] "No context, existence or lock check; masks IRQ and FIQ; not serialized against the spinlocked +0x6c/+0x70/+0x78/+0x7c" ["semaphore creation records A 0x4005dfd8 / B 0x400edf98 stride 16", "semaphore control blocks A 0x4005af0c / B 0x400eaecc stride 16"] [0x724974,0x7293b0] [dis A "13492-13516", dis B "13477-13501", contracts "sync-family"])
      , (0x78, recoveredAs (Just "pol_sem") "semaphore-poll" (classSelect ++ " in 1..domain+0x50 (250)") "r0 = 0 with the count decremented when it was nonzero" [(-25,"context word nonzero or CPSR.I set"),(-18,"invalid class or id"),(-42,"semaphore not created"),(-50,"count 0; nothing changed")] (taskCheck ++ " (domain+0x14 via +0x18/+0x1c); never blocks") ["semaphore control block", "creation record exists flag", "class spinlock A 0x400545c4 / B 0x400e4584"] [0x724ad8,0x7295b8] [dis A "7947-8025", dis B "7931-8009", contracts "sync-family"])
      , (0x84, recoveredAs Nothing "eventflag-delete" "r0: only uxth(r0) is used, index n-1 into the executing domain's 120-entry tables (per-task flags share the task index), class bits ignored, no range check" "r0 = 0 on every path; record reset to {attr 0, pattern 0, exists 0}, waiter list head reset to itself and pattern set to 0; queued waiters are dropped without being readied" [] "No context, existence or lock check; masks IRQ and FIQ" ["eventflag creation records A 0x4005d178 / B 0x400ed138 stride 12", "eventflag control blocks A 0x400624b4 / B 0x400f2474 stride 16"] [0x72612c,0x729900] [dis A "13555-13580", dis B "13540-13565", contracts "sync-family"])
      , (0x90, recoveredAs (Just "clr_flg") "eventflag-clear" (classSelect ++ " in 1..domain+0x58 (120); r1 bits kept") "r0 = 0 with pattern := pattern AND r1; waiters are not examined" [(-25,"context word nonzero or CPSR.I set"),(-18,"invalid class or id"),(-42,"flag not created")] (taskCheck ++ " (domain+0x14); never blocks") ["eventflag control block", "creation record exists flag", "class spinlock"] [0x724638,0x7246f0,0x729a8c] [dis A "8534-8611", dis B "8518-8595", contracts "sync-family"])
      , (0xc4, recoveredAs (Just "prcv_dtq") "data-queue-receive-poll" (classSelect ++ " in 1..dom+0x60; r1 output word pointer") "r0 = 0 with *r1 = the word taken from the ring buffer or directly from the first waiting sender (which is readied, locally or by peer notification)" [(-25,"context word nonzero or CPSR.I set"),(-18,"invalid class or id"),(-42,"queue not created"),(-50,"no waiting sender and buffer empty")] (taskCheck ++ "; also takes the sender domain's scheduler lock when a sender waits") ["data-queue records [dom+0x5c] stride 32", "creation records A 0x4005988c / B 0x400e984c", "ring buffer", "waiting sender T"] [0x725214] [dis A "10402-10560", dis B "10386-10544", contracts "object-family"])
      , (0x108, recoveredAs (Just "consistent with cre_cyc without parameter, id or context checks") "cyclic-handler-create" "r0: only uxth(r0) is used, index id-1 into the 64-entry table, class bits ignored, no range check; r1 pointer to five words {attribute, argument, handler, period, phase} copied verbatim" "r0 = 0 with the record copied and +0x14 exists set to 1" [(-41,"record already exists")] "No context check; masks IRQ and FIQ" ["cyclic creation table A 0x4005ef78 / B 0x400eef38 stride 24"] [0x725424] [dis A "13804-13839", dis B "13789-13824", contracts "object-family"])
      , (0x10c, recoveredAs (Just "consistent with del_cyc without checks") "cyclic-handler-delete" "r0: only uxth(r0) is used, no range check" "r0 = 0; all six record words zeroed; an armed timer entry stays queued unless +0x114 ran first" [] "No context check; masks IRQ and FIQ" ["cyclic creation table"] [0x7256c4] [dis A "13840-13860", dis B "13825-13845", contracts "object-family"])
      , (0x110, recoveredAs (Just "sta_cyc") "cyclic-handler-start" (classSelect ++ " in 1..dom+0x78 (64)") "r0 = 0; arms the runtime record for next clock (A 0x40054770 / B 0x400e4730) + period, removing an existing heap entry first when already active" [(-25,"context word nonzero or CPSR.I set"),(-18,"invalid class or id"),(-42,"handler not created")] (taskCheck ++ " (scheduler lock dom+0x8)") ["cyclic runtime table [dom+0x74] stride 24 (A 0x4005928c / B 0x400e924c)", "timer heap [dom+0x34]", "next clock"] [0x725424] [dis A "12249-12340", dis B "12233-12324", contracts "object-family"])
      , (0x114, recoveredAs (Just "stp_cyc") "cyclic-handler-stop" (classSelect ++ " in 1..dom+0x78 (64)") "r0 = 0, including when already stopped; clears the active flag and removes the heap entry" [(-25,"context word nonzero or CPSR.I set"),(-18,"invalid class or id"),(-42,"handler not created")] (taskCheck ++ " (scheduler lock dom+0x8)") ["cyclic runtime table", "timer heap"] [0x7256c4] [dis A "12341-12424", dis B "12325-12408", contracts "object-family"])
      , (0x11c, recoveredAs (Just "rot_rdq") "ready-queue-rotate" "r0 priority 0..16 unsigned; 0 selects the current task's priority" "r0 = 0; moves the head of that ready queue of the executing domain to its tail and dispatches when the scheduled task changed and dispatch is enabled" [(-25,"context word nonzero or CPSR.I set"),(-17,"r0 > 16")] "Task context, IRQ unmasked; executing domain's scheduler lock" ["ready queues [dom+0x20]", "scheduled T A 0x40054768 / B 0x400e4728", "dispatch-enabled word"] [0x723ce8] [dis A "12425-12480", dis B "12409-12464", contracts "object-family"])
      , (0x128, recoveredAs (Just "consistent with get_tid for task-context callers") "current-task-index-get" "r0 output word pointer" "r0 = 0 with *r0 = one-based index of the executing core's current T in its own table, no class bits" [(-25,"context word nonzero or CPSR.I set")] "Task context, IRQ unmasked; masks IRQ around the read; the non-task variant is +0x12c" ["current T A 0x40054760 / B 0x400e4720", "T base A 0x4005ff0c / B 0x400efecc"] [] [dis A "12654-12687", dis B "12638-12671", contracts "object-family"])
      , (0x140, recoveredAs (Just "dis_dsp") "dispatch-disable" "None read" "r0 = 0; writes 0 to the dispatch-enabled word once the current T is runnable" [(-25,"context word nonzero or CPSR.I set")] "Task context, IRQ unmasked; executing domain's scheduler lock" ["dispatch-enabled word A 0x40054764 / B 0x400e4724", "current T +0xc"] [0x723efc] [dis A "12893-12949", dis B "12877-12933", contracts "object-family"])
      , (0x144, recoveredAs (Just "ena_dsp") "dispatch-enable" "None read" "r0 = 0 after writing 1 to the dispatch-enabled word and dispatching to a different scheduled task; when the current T+0x30 context-discarded flag is set (the task was reset by termination while running with dispatch disabled) it enters the non-saving dispatcher and does not return" [(-25,"context word nonzero or CPSR.I set")] "Task context, IRQ unmasked; executing domain's scheduler lock" ["dispatch-enabled word", "current and scheduled T", "T+0x30 set by termination 0x45790, cleared by the start trampoline 0x4374c"] [0x723f7c] [dis A "12950-13019", dis B "12934-13003", dis A "3513", dis A "5577-5578", contracts "object-family"])
      , (0x104, partialAs (Just "consistent with isig_tim") "system-tick" "None read" "r0 = 0 after firing every expired timeout-heap entry and advancing the kernel clock by 10" [(-25,"context word 0 (task context)"),(-25,"CPSR.I set at entry")] "Handler context with IRQ unmasked; holds the tick root lock with IRQ masked across heap work and expiry callbacks; lock spin unbounded; callers are the interrupt-32 callbacks A 0x40b24 / B 0xd0b24" ["tick root A 0x400545c8 / B 0x400e4588", "timeout heap", "clock pair A 0x4005476c/0x40054770 / B 0x400e472c/0x400e4730"] [] [dis A "4937-5040", dis B "4921-5024", contracts "interrupt-family"])
      , (0x158, partialAs (Just "consistent with ena_int; failure status -5") "interrupt-enable" "r0 interrupt number, signed, 0..127 defined; negative values pass the check" "r0 = 0 after programming the executing core's controller bank (CP14 identity 0x4003 selects 0xfffb3000, otherwise 0xfffb4000) from the handler table and setting the enable bit" [(-5,"r0 > 127"),(-5,"no handler defined for r0")] "No context-word check; masks IRQ around table read and controller writes; bank follows the executing core, not the slot domain" ["handler table A 0x400538fc / B 0x400e38bc stride 24", "interrupt controller banks 0xfffb3000 / 0xfffb4000"] [0x723870,0x72ae44] [dis A "1069-1075", dis B "1069-1075", dis A "2729-2964", contracts "interrupt-family"])
      , (0x15c, partialAs (Just "consistent with dis_int; failure status -5") "interrupt-disable" "r0 interrupt number, signed, 0..127 defined; negative values pass the check" "r0 = 0 after clearing the source's enable/configuration bits in the executing core's controller bank and pointing its vector at the default empty handler; the handler table is untouched" [(-5,"r0 > 127")] "No context-word check; masks IRQ around the controller writes" ["interrupt controller banks 0xfffb3000 / 0xfffb4000"] [0x7239a0] [dis A "1076-1082", dis B "1076-1082", dis A "2965-3090", contracts "interrupt-family"])
      , (0x160, recoveredAs (Just "def_inh") "interrupt-handler-define" "r0 interrupt number (uxth, 0..127); r1 NULL or pointer to five words {ignored, handler nonzero, 0..15, 0..1, 0..3}" "r0 = 0 with table entry words +0..+0x10 written; NULL installs {default empty handler, 0, 15, 0, 1} and returns 0 even for an id above 127" [(-17,"non-null r1 with id > 127, handler 0 or a field out of range")] "No context-word check; masks IRQ and FIQ; writes no controller register" ["handler table A 0x400538fc / B 0x400e38bc stride 24"] [0x723870,0x7239a0,0x72ae44] [dis A "13861-13891", dis B "13846-13876", dis A "2644-2719", contracts "interrupt-family"])
      , (0x164, c "irq-save" "No semantic arguments recovered" "Prior IRQ mask for paired restoration" [] "CPU-local selected root; task/IRQ nesting closure incomplete" ["CPSR.I"] [0x7235d8,0x726098] criticalEvidence)
      , (0x168, c "irq-restore" "Prior mask returned by +0x164" "Caller observes restored IRQ mask; return register contract incomplete" [] "Paired on same selected root" ["CPSR.I"] [0x7236b0,0x726098] criticalEvidence)
      , (0x16c, c "current-task-query" "Output pointer through image4 adapter; exact lower tuple incomplete" "Status; wrapper0x723cb8 returns identity0 on failure" [(-25,"masked IRQ or nonzero context")] "Unmasked IRQ/task context, valid current node" ["current T", "C", "per-core context"] [0x723cb8] ["reports/replacement-inventory-baseline.md:1398-1408", "reports/stock-heap-survey.md:95-99"])
      , (0x170, c "message-buffer-acquire" "Pool/timeout arguments from send; complete tuple unknown" "Buffer/status; failure ownership incomplete" [] "Context rules incomplete" ["message pool", "buffer ownership"] [0x72503c] queueEvidence)
      , (0x1618, c "core-identity" "No semantic arguments" "0 exactly for CP14 c2 discriminator0x4003; otherwise1" [] "CPU-local CP14 observation; physical identity qualification separate" ["CP14 c2"] [0x7235d8,0x7236b0,0x723cb8,0x724480,0x7247ac,0x724cdc,0x72503c,0x725318,0x725dd8,0x726098] taskEvidence)
      , (0x161c, c "critical-lock-retry" "Private critical adapter arguments not fully recovered here" "Retry/identity-dependent operation; complete result unknown" [] "Critical-pair contract only" ["shared lock", "per-core critical counter"] [0x7235d8] criticalEvidence)
      , (0x1620, c "critical-lock-release" "Private critical adapter arguments not fully recovered here" "Complete result unknown" [] "Critical-pair contract only" ["shared lock", "per-core critical counter"] [0x7236b0] criticalEvidence)
      , (0x1624, c "critical-lock-observe" "Private critical adapter arguments not fully recovered here" "Complete result unknown" [] "Critical-pair contract only" ["shared lock", "per-core critical counter"] [0x7235d8,0x7236b0] criticalEvidence)
      ]

main :: IO ()
main = do
  args <- getArgs
  case args of
    [publisherA, publisherB, loaderA, loaderB, listingA, image4, surfacePath, boundaryPath, output] -> do
      exists <- doesPathExist output
      unless (not exists) (fail "output already exists; use a fresh evidence path")
      publisherBytesA <- B.readFile publisherA
      publisherBytesB <- B.readFile publisherB
      let sourceA = C.unpack publisherBytesA
          sourceB = C.unpack publisherBytesB
      entriesA <- either fail pure (publisherEntries sourceA)
      entriesB <- either fail pure (publisherEntries sourceB)
      unless (map fst entriesA == map fst entriesB) (fail "A/B published slot offsets differ")
      bytesA <- B.readFile loaderA
      bytesB <- B.readFile loaderB
      unless (B.length bytesA `mod` 4 == 0 && B.length bytesB `mod` 4 == 0) (fail "unaligned loader input")
      listingBytes <- B.readFile listingA
      imageBytes <- B.readFile image4
      unless (digest bytesA == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (fail "loader A is not canonical X-M1 1.50")
      unless (digest bytesB == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec") (fail "loader B is not canonical X-M1 1.50")
      unless (digest imageBytes == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16") (fail "image4 is not canonical X-M1 1.50 base0x61c400")
      let exportedA = functionStarts (C.unpack listingBytes)
          callers = instructions 0x61c400 imageBytes
      surfaceBytes <- B.readFile surfacePath
      surfaceValue <- either fail pure (JSON.decodeStrict surfaceBytes)
      surfaces <- either fail pure (parseEither parseJSON surfaceValue :: Either String SurfaceMap)
      boundaryBytes <- B.readFile boundaryPath
      boundaryValue <- either fail pure (JSON.decodeStrict boundaryBytes)
      reviewed <- either fail pure (boundaries <$> (parseEither parseJSON boundaryValue :: Either String FunctionBoundaries))
      let loaderOf domain = if domain == A then (0x40000, bytesA) else (0xd0000, bytesB)
          admitBoundary boundary = do
            let (base, bytes) = loaderOf (boundaryDomain boundary)
                FunctionStart start = boundaryStart boundary
            unless (loaderSha256 boundary == Sha256 (digest bytes)) (Left ("boundary " ++ hex start ++ " names a different loader image"))
            unless (start >= base && start < base + fromIntegral (B.length bytes) && start `mod` 4 == 0) (Left ("boundary " ++ hex start ++ " is not an aligned address of its loader"))
            unless (not (null (boundaryEvidence boundary))) (Left ("boundary " ++ hex start ++ " has no evidence"))
            pure (boundaryDomain boundary, start)
      admitted <- either fail pure (mapM admitBoundary reviewed)
      let boundaryStarts domain = [start | (owner, start) <- admitted, owner == domain]
          makeDomain domain base bytes slotEntries starts =
            let decoded = instructions base bytes
                fences = sort (nub (starts ++ [target | (_, A.Branch _ True target) <- decoded, target >= base, target < base+fromIntegral (B.length bytes)] ++ [address | (_, EntryAddress address) <- slotEntries]))
            in object
              [ "domain" .= show domain, "root" .= hex (if domain == A then 0x4004dae0 else 0x400ddaa0)
              , "slots" .= [let info = contract offset in object
                  [ "offset" .= hex off, "target" .= hex address, "recovery" .= case recovery info of { Recovered -> ("recovered" :: String); Partial -> "partial"; Unknown -> "unknown" }
                  , "operation" .= operation info, "analogue" .= analogue info, "arguments" .= arguments info, "result" .= result info
                  , "errors" .= [object ["value" .= status,"meaning" .= meaning] | (status,meaning) <- errors info]
                  , "context" .= context info, "objects" .= objects info
                  , "directImage4Calls" .= [hex site | (site,A.Branch _ True target) <- callers,target == address]
                  , "computedWrapperEntries" .= map hex (computedWrappers info)
                  , "callerClosure" .= ("open: named wrappers are source-evidenced lower bounds, not exhaustive computed/Thumb/tail/alias closure; linear BL matches may be data" :: String)
                  , "extent" .= extent base bytes fences entry
                  , "evidence" .= evidence info
                  ] | (offset@(SlotOffset off),entry@(EntryAddress address)) <- slotEntries]
              ]
      L.writeFile output (encode (object
        [ "schemaVersion" .= (1 :: Int), "firmware" .= ("X-M1 1.50" :: String)
        , "scope" .= ("Source inventory; unknowns and extent refusals block complete RTOS replacement. No new guest qualification." :: String)
        , "inputs" .= [InputIdentity path (B.length bytes) (digest bytes) | (path,bytes) <- [(publisherA,publisherBytesA),(publisherB,publisherBytesB),(loaderA,bytesA),(loaderB,bytesB),(listingA,listingBytes),(image4,imageBytes),(surfacePath,surfaceBytes),(boundaryPath,boundaryBytes)]]
        , "extentMethod" .= ("StockInventory.deriveFunction; no external tails admitted; fence=min(next published entry, direct BL target, A Ghidra export, reviewed function boundary bound to the same domain loader digest, image end). A exports and boundaries never transferred to B. Refusals preserve missing extents." :: String)
        , "functionBoundaries" .= [object
            [ "domain" .= show (boundaryDomain boundary), "start" .= hex start
            , "loaderSha256" .= (let Sha256 text = loaderSha256 boundary in text)
            , "evidence" .= boundaryEvidence boundary, "basis" .= basis boundary
            ] | boundary <- reviewed, let FunctionStart start = boundaryStart boundary]
        , "domains" .= [makeDomain A 0x40000 bytesA entriesA (exportedA ++ boundaryStarts A),makeDomain B 0xd0000 bytesB entriesB (boundaryStarts B)]
        , "surfaceMap" .= surfaces
        ]) <> "\n")
    _ -> fail "usage: rtos-manifest PUBLISHER_A PUBLISHER_B LOADER_A LOADER_B GHIDRA_A IMAGE4 SURFACE_JSON BOUNDARIES_JSON OUTPUT_JSON"
