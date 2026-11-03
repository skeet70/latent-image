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
    unknown = Contract "unknown" Nothing Nothing [] Nothing [] []
      ["replacement/primary_initializer/src.rs:25-49,93-96", "replacement/interface_initializer/src.rs:25-49,78-81"] Unknown
    c name args returned failures required touched wrappers citations = Contract name (Just args) (Just returned) failures (Just required) touched wrappers citations Partial
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
      , (0x148, c "message-context-selector" "No recovered complete tuple" "Selects send variant; numeric result contract unknown" [] "Queries current execution context" ["per-core context"] [0x72503c] queueEvidence)
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
                  , "operation" .= operation info, "arguments" .= arguments info, "result" .= result info
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
