{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson as A
import qualified Data.Aeson.Key as K
import qualified Data.Aeson.KeyMap as KM
import Data.Aeson.Types (parseMaybe)
import Data.Bits (shiftR, (.&.), (.|.))
import qualified Data.ByteString as B
import qualified Data.ByteString.Char8 as C
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft)
import Data.List (zip5)
import Data.Maybe (fromMaybe)
import qualified Data.Map.Strict as M
import qualified Data.Text as T
import qualified Data.Vector as V
import Data.Word (Word32, Word8)
import Emulation.PropertyPairCheck
import Emulation.PropertyPairPlan
import Emulation.PropertyPairRoutes
import Emulation.Sha256 (sha256, sha256Hex)
import EmulatorUsb (CrawlKey (..))
import Numeric (showHex)
import System.Exit (exitFailure)

check :: String -> Bool -> IO Bool
check name passed = do
  unless passed (putStrLn ("FAIL " ++ name))
  pure passed

digestOf :: B.ByteString -> String
digestOf = sha256Hex . sha256

hex :: Word32 -> String
hex value = "0x" ++ showHex value ""

littleWords :: [Word32] -> B.ByteString
littleWords values = B.pack [fromIntegral (value `shiftR` (8 * index)) | value <- values, index <- [0 .. 3 :: Int]]

putWord :: Word32 -> Word32 -> B.ByteString -> B.ByteString
putWord address value bytes = let offset = fromIntegral (address - image4Base) in
  B.take offset bytes <> littleWords [value] <> B.drop (offset + 4) bytes

-- A synthetic image4, carrier and contract.

image4Base, carrierStart, carrierEnd, fillWord, rustBody, retiredGetter, retiredSetter, getterCall, setterCall :: Word32
image4Base = 0x61c400
carrierStart = 0x62c4b0
carrierEnd = 0x62c530
fillWord = 0xe7f000f0
rustBody = 0x62c4f0
retiredGetter = 0x621100
retiredSetter = 0x622100
getterCall = 0x620000
setterCall = 0x620010

branchLink :: Word32 -> Word32 -> Word32
branchLink site target = 0xeb000000 .|. (((target - (site + 8)) `shiftR` 2) .&. 0xffffff)

-- | The four store accessors, then one body word.
linked :: B.ByteString
linked = littleWords
  [ 0xe30ccc64, 0xe340c096, 0xe7cc1000, 0xe12fff1e
  , 0xe30ccc64, 0xe340c096, 0xe18c10b0, 0xe12fff1e
  , 0xe30ecc00, 0xe340c089, 0xe7cc1000, 0xe12fff1e
  , 0xe30ecc00, 0xe340c089, 0xe18c10b0, 0xe12fff1e
  , 0xe1a00000 ]

storeSite :: StoreAccessor -> Word32
storeSite accessor = carrierStart + 16 * fromIntegral (fromEnum accessor) + 8

canonical4 :: B.ByteString
canonical4 = foldr (uncurry putWord) (B.replicate 0x20000 0)
  [ (getterCall, branchLink getterCall getterEntry), (setterCall, branchLink setterCall setterEntry)
  , (0x623000, retiredGetter), (0x623004, retiredSetter) ]

staged4 :: B.ByteString
staged4 = foldr (uncurry putWord) spliced [(0x623000, carrierStart), (0x623004, carrierStart + 0x10)]
 where
  offset address = fromIntegral (address - image4Base)
  fill = littleWords (replicate ((fromIntegral (carrierEnd - carrierStart) - B.length linked) `div` 4) fillWord)
  spliced = B.take (offset carrierStart) canonical4 <> linked <> fill <> B.drop (offset carrierEnd) canonical4

otherImages :: [(ImageName, Word32)]
otherImages = [(Image5, 0xbf1a00), (Plain, 0x1100000), (LoaderA, 0x40000), (LoaderB, 0xd0000)]

images :: M.Map ImageName B.ByteString
images = M.fromList ((Image4, canonical4) : [(name, B.replicate 16 0) | (name, _) <- otherImages])

sourceDat, ownedDat, symbols, observer :: String
sourceDat = replicate 64 'a'
ownedDat = replicate 64 'b'
symbols = replicate 64 'c'
observer = replicate 64 'd'

contractBytes :: B.ByteString
contractBytes = BL.toStrict (A.encode (object
  [ "schema" .= (1 :: Int)
  , "carrier" .= object ["start" .= carrierStart, "end" .= carrierEnd, "fill_word" .= fillWord]
  , "patch" .= object ["words" .=
      [ object ["address" .= (0x623000 :: Word32), "original" .= retiredGetter, "patched" .= carrierStart, "property" .= (5 :: Int), "table" .= ("getter" :: T.Text)]
      , object ["address" .= (0x623004 :: Word32), "original" .= retiredSetter, "patched" .= (carrierStart + 0x10), "property" .= (5 :: Int), "table" .= ("setter" :: T.Text)] ]]
  , "classification" .= object ["properties" .= [property index | index <- [0 .. 644 :: Word32]]]
  , "source_sha256" .= object ["dat" .= sourceDat]
  , "scan" .= object ["images" .= (scanImage "image4" image4Base canonical4 : [scanImage (imageNameWire name) base (images M.! name) | (name, base) <- otherImages])]
  , "candidate_dat_sha256" .= ownedDat
  , "linked_sha256" .= digestOf linked
  , "linked_bytes" .= B.length linked
  , "symbols_sha256" .= symbols
  , "staged_image4_sha256" .= digestOf staged4
  , "stack" .= object ["entries" .= [object ["name" .= storeAccessorName accessor, "entry" .= (storeSite accessor - 8)] | accessor <- [minBound .. maxBound]]]
  ]))
 where
  scanImage name base bytes = object ["image" .= (name :: T.Text), "base" .= base, "bytes" .= B.length bytes, "sha256" .= digestOf bytes]
  property index = let (kind, getter, setter) = arms index in
    object ["class" .= (kind :: T.Text), "property" .= hex index, "getterArm" .= hex getter, "setterArm" .= hex setter]
  arms index
    | index == 5 = ("owned-field", retiredGetter, retiredSetter)
    | index == 6 = ("delegated", 0x621140, 0x622200)
    | otherwise = ("delegated", 0x621000, 0x622000)

crawlRoute :: RouteName
crawlRoute = RouteName "crawl-trial-00001"

routesBytes :: B.ByteString
routesBytes = routesWith [SetterEffect 5, CommitEffect]

-- | A route file whose crawl route records the given effects.
routesWith :: [CrawlEffect] -> B.ByteString
routesWith effects = BL.toStrict (encodeRouteFile (RouteFile (replicate 64 '0') "test" CrawlEffectsSource []
  (phaseRoutes ++ [Route crawlRoute (KeyRoute [KeyOk] (CrawlTrial 1 effects))])))

bindings :: Bindings
bindings = Bindings sourceDat ownedDat routesBytes contractBytes linked symbols images observer

-- Synthetic observations.

type Regions = M.Map Word32 B.ByteString

zeroRegions :: Regions
zeroRegions = M.fromList [(rangeStart range, B.replicate (fromIntegral (rangeEnd range - rangeStart range)) 0) | range <- settingsRanges]

poke :: [(Word32, Word8)] -> Regions -> Regions
poke bytes regions = foldl set regions bytes
 where
  set current (address, byte) = case M.lookupLE address current of
    Just (start, region) | address - start < fromIntegral (B.length region) ->
      let offset = fromIntegral (address - start) in M.insert start (B.take offset region <> B.singleton byte <> B.drop (offset + 1) region) current
    _ -> current

regionsValue :: Regions -> Value
regionsValue regions = toArray [object ["start" .= start, "bytes" .= concatMap byteHex (B.unpack bytes)] | (start, bytes) <- M.toList regions]
 where
  byteHex byte = let text = showHex byte "" in replicate (2 - length text) '0' ++ text

toArray :: [Value] -> Value
toArray = A.toJSON

hit :: SiteLabel -> Word32 -> [Word32] -> Word32 -> Word32 -> Value
hit label address arguments sp lr = object
  [ "site" .= siteLabelWire label, "address" .= hex address, "core" .= (0 :: Int), "instructions" .= (0 :: Int)
  , "registers" .= object [K.fromString register .= hex value | (register, value) <- zip ["r0", "r1", "r2", "r3"] (arguments ++ repeat 0)]
  , "sp" .= hex sp, "lr" .= hex lr, "cpsr" .= (0 :: Int) ]

cpuWrite :: Word32 -> Int -> Maybe Integer -> [Word8] -> Word32 -> Value
cpuWrite address width value before pc = object
  [ "source" .= ("cpu-write" :: T.Text), "address" .= address, "physical" .= address, "size" .= width
  , "value" .= value, "before" .= before, "byte_order" .= ("little" :: T.Text), "core" .= (0 :: Int)
  , "pc" .= pc, "instructions" .= (0 :: Int), "tick" .= (0 :: Int) ]

hostWrite :: Word32 -> Int -> Value
hostWrite address width = object ["source" .= ("host-write" :: T.Text), "physical" .= address, "size" .= width]

watch :: T.Text -> [Value] -> Value
watch views rows = object
  [ "enabled" .= True, "ranges" .= [[rangeStart range, rangeEnd range] | range <- settingsRanges]
  , "records" .= rows, "violation" .= Null, "classes" .= ([] :: [Value]), "views" .= views ]

-- | Step n of a chain presses OK n times from the attached state; `stepBefore` overrides its
-- parent's after regions.
data Step = Step { stepHits :: [Value], stepRows :: [Value], stepAfter :: Regions, stepStatus :: T.Text, stepMenu :: Value, stepBefore :: Maybe Regions }

observation :: Role -> Step -> Observation
observation role step = chain role [step]

chain :: Role -> [Step] -> Observation
chain = chainWith routesBytes

chainWith :: B.ByteString -> Role -> [Step] -> Observation
chainWith routes role chained = Observation receipt spec reset hits steps session
 where
  plan = either error id (derivePlan (PlanInputs role (either error id (decodeContract contractBytes)) images linked Nothing))
  spec = BL.toStrict (A.encode (specValue plan))
  segment phase = object ["phase" .= phase, "instructions" .= (0 :: Int), "hits_end" .= (0 :: Int), "watch" .= watch "observe" [], "regions" .= regionsValue zeroRegions]
  reset = BL.toStrict (A.encode (object
    [ "schema" .= (1 :: Int), "reset" .= object ["instructions" .= (0 :: Int)], "regions_at_reset" .= regionsValue zeroRegions
    , "segments" .= [segment phase | phase <- [minBound .. maxBound :: AttachPhase]] ]))
  hits = B.concat [BL.toStrict (A.encode value) <> "\n" | value <- concatMap stepHits chained]
  ends = scanl (+) 0 (map (length . stepHits) chained)
  befores = zipWith fromMaybe (zeroRegions : map stepAfter chained) (map stepBefore chained)
  steps = BL.toStrict (A.encode [ object
    [ "step" .= number, "parent" .= (number - 1), "keys" .= replicate number KeyOk, "hits" .= [from, to]
    , "idle" .= object ["instructions" .= (0 :: Int), "ran" .= instructionsValue preKeyIdle]
    , "press" .= object ["failure" .= Null, "settle" .= object ["status" .= stepStatus step], "menu" .= stepMenu step]
    , "watch" .= watch "fixed" (stepRows step), "regions_before" .= regionsValue before, "regions_after" .= regionsValue (stepAfter step) ]
    | (number, step, before, from, to) <- zip5 [1 :: Int ..] chained befores ends (drop 1 ends) ])
  session = digestOf "session"
  image4 = if role == RetainedRole then canonical4 else staged4
  digest bytes = object ["sha256" .= digestOf bytes]
  same text = object ["sha256" .= (text :: String)]
  receipt = BL.toStrict (A.encode (object
    [ "role" .= roleWire role
    , "inputs" .= object
        [ "dat" .= same (if role == RetainedRole then sourceDat else ownedDat), "routes" .= digest routes
        , "contract" .= digest contractBytes, "linked" .= digest linked, "symbols" .= same symbols, "observer" .= same observer
        , "run" .= digest "run", "boot_rom" .= digest "rom", "adjustment_nand" .= digest "nand", "bridge" .= digest "bridge"
        , "analysis" .= object [K.fromText (imageNameWire name) .= digestOf bytes | (name, bytes) <- M.toList images]
        , "role_image4_sha256" .= digestOf image4 ]
    , "files" .= object
        [ "spec.json" .= digestOf spec, "reset-observation.json" .= digestOf reset, "hits.jsonl" .= digestOf hits
        , "steps.json" .= digestOf steps, "emulator-session.json" .= session ]
    , "outcome" .= object ["status" .= ("completed" :: T.Text)]
    , "bounds" .= object ["hit_limit" .= hitLimit, "watch_limit" .= watchLimit]
    , "probe" .= Null
    , "pre_key_idle_instructions" .= instructionsValue preKeyIdle ]))

stackPointer, written :: Word32
stackPointer = 0x1000
written = 0x96cc74

setterIn, commit, setterOut, getterIn, getterOut :: Value
setterIn = hit SetterEntry setterEntry [5, 1] stackPointer (setterCall + 4)
commit = hit PropertyCommit propertyCommit [] stackPointer 0
setterOut = hit SetterReturn (setterCall + 4) [0] stackPointer 0
getterIn = hit GetterEntry getterEntry [5] stackPointer (getterCall + 4)
getterOut = hit GetterReturn (getterCall + 4) [0xab] stackPointer 0

storeHit :: StoreAccessor -> Word32 -> Word32 -> Value
storeHit accessor offset value = hit (AccessorStore accessor) (storeSite accessor) [offset, value] stackPointer 0

menu :: Value
menu = object ["node" .= (1 :: Int)]

-- | The owned setter stores 0xab through the bank accessor; the retained one from its stock arm.
ownedStep, retainedStep :: Step
ownedStep = Step [setterIn, commit, storeHit BankStore8 0x10 0xab, setterOut, getterIn, getterOut]
  [cpuWrite written 1 (Just 0xab) [0] (storeSite BankStore8)] (poke [(written, 0xab)] zeroRegions) "packet-consumed" menu Nothing
retainedStep = ownedStep
  { stepHits = [setterIn, commit, setterOut, getterIn, getterOut]
  , stepRows = [cpuWrite written 1 (Just 0xab) [0] retiredSetter] }

qualified :: Role -> Step -> Either Refusal Qualified
qualified role step = qualify bindings role (observation role step)

refusal :: Role -> Step -> Maybe Refusal
refusal role step = either Just (const Nothing) (qualified role step)

crawlResult :: Role -> Step -> Maybe RouteResult
crawlResult role step = either (const Nothing) (lookup crawlRoute . map (\result -> (resultRoute result, result)) . qualifiedRoutes) (qualified role step)

compared :: Step -> Step -> Either Refusal ()
compared retained owned = do
  left <- qualified RetainedRole retained
  right <- qualified OwnedRole owned
  () <$ compareRoles left right

-- | A crawl route's comparison from its ordered setter calls alone.
comparedSetters :: [Call] -> [Call] -> Either Refusal [(RouteResult, RouteResult)]
comparedSetters retained owned = compareRoles (qualifiedWith RetainedRole retained) (qualifiedWith OwnedRole owned)
 where
  qualifiedWith role setters = Qualified role [] 0 1 [RouteResult crawlRoute CrawlClass [] setters [] [] [] [] [] []]

-- | The retained crawl route's effect block runs when its route file records `recorded` and its
-- step's hits end with `extra` after the standard setter, commit and getter calls.
effectBlocksWith :: [CrawlEffect] -> [Value] -> Either Refusal [(Int, Int)]
effectBlocksWith recorded extra = do
  let routes = routesWith recorded
  result <- qualify bindings { bindRoutes = routes } RetainedRole (chainWith routes RetainedRole [retainedStep { stepHits = stepHits retainedStep ++ extra }])
  pure (concat [resultEffectBlocks route | route <- qualifiedRoutes result, resultRoute route == crawlRoute])

-- | A setter entry for the property and its return.
setterPair :: Word32 -> [Value]
setterPair property = [hit SetterEntry setterEntry [property, 0] stackPointer (setterCall + 4), hit SetterReturn (setterCall + 4) [0] stackPointer 0]

blockHits :: [Value]
blockHits = concatMap setterPair timeDrivenSequence

-- | The retained and owned setter block runs of the first compared route, as (position, first call, repeats).
blockRunsOf :: [(RouteResult, RouteResult)] -> Maybe ([(Int, Int, Int)], [(Int, Int, Int)])
blockRunsOf pairs = case comparisonValue pairs of
  Object top | Just (Array routes) <- KM.lookup "per_route" top, Just (Object route) <- routes V.!? 0
             , Just (Object runs) <- KM.lookup "setter_block_runs" route ->
    (,) <$> (KM.lookup "retained" runs >>= decodeRuns) <*> (KM.lookup "owned" runs >>= decodeRuns)
  _ -> Nothing
 where
  decodeRuns = parseMaybe (A.withArray "runs" (mapM (A.withObject "run" (\run -> (,,) <$> run A..: "position" <*> run A..: "first_call" <*> run A..: "repeats")) . V.toList))

blockCall :: Word32 -> Word32 -> Call
blockCall value property = Call [property, value, 0, 1] 0x630a04 (Just 1)

timeBlock, changedBlock :: [Call]
timeBlock = map (blockCall 0) timeDrivenSequence
changedBlock = map (blockCall 1) timeDrivenSequence

otherSetter :: Call
otherSetter = Call [5, 1, 0, 0] (setterCall + 4) (Just 0)

retainedSidecars :: Role -> T.Text -> RetainedObservation
retainedSidecars role status = RetainedObservation receipt (obsSteps base) (obsHits base)
 where
  base = observation role retainedStep
  receipt = case A.eitherDecodeStrict' (obsReceipt base) of
    Right (Object fields) -> BL.toStrict (A.encode (Object (KM.insert "outcome" (object ["status" .= status]) fields)))
    _ -> error "test receipt is not an object"

refusedAs :: Either Refusal a -> Refusal -> Bool
refusedAs result expected = either (== expected) (const False) result

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
    , check "an edge leaving the screen PLAY opened is left out and listed" (playbackTrials == [18])
    , check "an edge whose only effects are the time-driven block is left out and listed, one with another setter is kept" (timeDrivenTrials == [21])
    , check "an edge without a property effect is not a route" (all ((/= [KeyOk]) . routeKeys) selected)
    , check "a crawl route keeps its edge's effects" (map routeBody (take 1 selected) == [KeyRoute [KeyOk, KeyOk] (CrawlTrial 7 [SetterEffect 4, CommitEffect])])
    , check "route files round-trip with either effects source"
        (and [roundTrip source valid == Right (file source valid) | source <- [CrawlEffectsSource, retainedSource]])
    , check "a route file without the attach phases first is refused" (isLeft (roundTrip CrawlEffectsSource (selected ++ phaseRoutes)))
    , check "a key route needs a key" (isLeft (decodeRouteFile (C.pack (concat
        [ "{\"schema\":1,\"crawl_graph_sha256\":\"0\",\"selection\":\"t\",\"effects_source\":{\"kind\":\"crawl\"},\"exclusions\":[],\"routes\":["
        , "{\"name\":\"boot\",\"phase\":\"first-start\"},{\"name\":\"start-menu\",\"phase\":\"live-view\"},"
        , "{\"name\":\"attach\",\"phase\":\"attached\"},{\"name\":\"x\",\"keys\":[],\"crawl_trial\":1,\"effects\":[{\"kind\":\"commit\"}]}]}" ]))))
    , check "repeated route names are refused" (isLeft (roundTrip CrawlEffectsSource (phaseRoutes ++ selected ++ take 1 selected)))
    , check "an effects source with a key its kind does not have is refused"
        (isLeft (A.eitherDecodeStrict' @EffectsSource (C.pack "{\"kind\":\"crawl\",\"receipt_sha256\":\"00\"}")))
    , check "a crawl route without effects is refused from the crawl and admitted from a retained observation"
        (let silent = phaseRoutes ++ [Route crawlRoute (KeyRoute [KeyOk] (CrawlTrial 1 []))]
         in isLeft (roundTrip CrawlEffectsSource silent) && roundTrip retainedSource silent == Right (file retainedSource silent))

    , check "a retained observation's last step supplies each crawl route's effects"
        (fmap (map routeBody . snd) (retainedEffects (retainedSidecars RetainedRole "completed") [Route crawlRoute (KeyRoute [KeyOk] (CrawlTrial 1 [CommitEffect]))])
          == Right [KeyRoute [KeyOk] (CrawlTrial 1 [SetterEffect 5, CommitEffect])])
    , check "effects are refused from an owned or uncompleted observation, or one without a route's last step"
        (let routes = [Route crawlRoute (KeyRoute [KeyOk] (CrawlTrial 1 [CommitEffect]))]
             refused sidecars chosen = either Just (const Nothing) (retainedEffects sidecars chosen)
         in refused (retainedSidecars OwnedRole "completed") routes == Just (ObservationNotRetained OwnedRole)
              && refused (retainedSidecars RetainedRole "refused") routes == Just (ObservationNotCompleted "refused")
              && refused (retainedSidecars RetainedRole "completed") [Route crawlRoute (KeyRoute [KeyDown] (CrawlTrial 1 [CommitEffect]))]
                   == Just (LastStepMissing crawlRoute))

    , check "the owned observation qualifies with its store classed as an accessor write and its calls returned"
        (fmap (\result -> (map effectWriter (resultWrites result), map callResult (resultSetters result), map callResult (resultGetters result))) (crawlResult OwnedRole ownedStep)
          == Just ([OwnedAccessor], [Just 0], [Just 0xab]))
    , check "retained and owned observations of the same effects compare equal" (compared retainedStep ownedStep == Right ())

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
    , check "the next live arm and the retained image carry no guard"
        (refusal OwnedRole (guardHit (retiredGetter + 0x40)) == Just (UnarmedSite 6)
          && refusal RetainedRole retainedStep { stepHits = stepHits retainedStep ++ [hit (Guard RetiredGetterArm) retiredGetter [] stackPointer 0] } == Just (UnarmedSite 5))

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
             refusedWith key value = either Just (const Nothing) (qualify bindings OwnedRole (edited key value))
         in refusedWith "probe" (object ["path" .= ["right" :: T.Text]]) == Just (ReceiptShape "probe")
              && refusedWith "pre_key_idle_instructions" (A.toJSON (0 :: Int)) == Just (ReceiptShape "pre-key idle"))
    , check "a step without the pre-key idle is refused"
        (let base = observation OwnedRole ownedStep
             withoutIdle = case A.eitherDecodeStrict' (obsSteps base) of
               Right (Array steps) -> BL.toStrict (A.encode (Array (fmap (\step -> case step of
                 Object fields -> Object (KM.insert "idle" Null fields)
                 other -> other) steps)))
               _ -> error "test steps are not an array"
             edited = base { obsSteps = withoutIdle }
             rebound = edited { obsReceipt = case A.eitherDecodeStrict' (obsReceipt edited) of
               Right (Object fields) -> BL.toStrict (A.encode (Object (KM.insert "files" (object
                 [ "spec.json" .= digestOf (obsSpec edited), "reset-observation.json" .= digestOf (obsReset edited)
                 , "hits.jsonl" .= digestOf (obsHits edited), "steps.json" .= digestOf withoutIdle
                 , "emulator-session.json" .= obsSessionDigest edited ]) fields)))
               _ -> error "test receipt is not an object" }
         in case qualify bindings OwnedRole rebound of
              Left (StepShape 1 _) -> True
              _ -> False)
    , check "a crawl route whose last step lacks its recorded effects is refused"
        (refusal OwnedRole (ownedWith (filter (/= commit) (stepHits ownedStep))) == Just (RouteEffectsDiffer crawlRoute))
    , check "recorded and observed effects that differ only in a time-driven block's repeat count qualify, with the observed count"
        (effectBlocksWith ([SetterEffect 5, CommitEffect] ++ map SetterEffect timeDrivenSequence) (blockHits ++ blockHits ++ blockHits) == Right [(2, 3)])
    , check "recorded and observed effects that differ outside the block are refused"
        (effectBlocksWith ([SetterEffect 5, CommitEffect] ++ map SetterEffect timeDrivenSequence) (blockHits ++ setterPair 6 ++ blockHits)
           == Left (RouteEffectsDiffer crawlRoute)
          && effectBlocksWith ([SetterEffect 5, CommitEffect] ++ map SetterEffect timeDrivenSequence) (setterPair 6 ++ blockHits)
               == Left (RouteEffectsDiffer crawlRoute))

    , check "getter calls compare per (caller, arguments) as result sequences with repeats collapsed"
        (compared retainedStep (ownedWith (stepHits ownedStep ++ [getterIn, getterOut])) == Right ()
          && compared retainedStep (ownedWith (take 5 (stepHits ownedStep) ++ [hit GetterReturn (getterCall + 4) [0xac] stackPointer 0])) == Left (GetterResultsDiffer crawlRoute))
    , check "a getter result returned out of order is a difference even when the result set is equal"
        (let returning value = hit GetterReturn (getterCall + 4) [value] stackPointer 0
         in compared (retainedStep { stepHits = stepHits retainedStep ++ [getterIn, returning 0xac] })
                     (ownedWith (stepHits ownedStep ++ [getterIn, returning 0xac, getterIn, returning 0xab])) == Left (GetterResultsDiffer crawlRoute)
            && compared (retainedStep { stepHits = stepHits retainedStep ++ [getterIn, returning 0xac] })
                        (ownedWith (stepHits ownedStep ++ [getterIn, returning 0xac, getterIn, returning 0xac])) == Right ())
    , check "getter calls under different arguments interleave freely"
        (let other = hit GetterEntry getterEntry [6] stackPointer (getterCall + 4)
             returning value = hit GetterReturn (getterCall + 4) [value] stackPointer 0
         in compared (retainedStep { stepHits = stepHits retainedStep ++ [other, returning 0xcd] })
                     (ownedWith (stepHits ownedStep ++ [other, returning 0xcd, getterIn, getterOut])) == Right ())
    , check "a write that leaves its preimage is not compared"
        (compared retainedStep ownedStep { stepRows = stepRows ownedStep ++ [cpuWrite 0x96cc80 1 (Just 0) [0] retiredSetter] } == Right ())
    , check "settle status differs only through an exhausted budget; the menu always counts"
        (compared retainedStep ownedStep { stepStatus = "budget-exhausted" } == Right ()
          && compared retainedStep ownedStep { stepMenu = object ["node" .= (2 :: Int)] } == Left (PressDiffers crawlRoute))
    , check "a child step continues from its parent step's after regions"
        (let child = retainedStep { stepHits = [], stepRows = [] } in
          either Just (const Nothing) (qualify bindings RetainedRole (chain RetainedRole [retainedStep, child])) == Nothing
            && either Just (const Nothing) (qualify bindings RetainedRole (chain RetainedRole [retainedStep, child { stepBefore = Just zeroRegions }]))
                 == Just (RegionContinuity (StepInterval 2)))

    , check "setter sequences that differ only in a time-driven block's repeat count compare equal and report the counts"
        (fmap blockRunsOf (comparedSetters (otherSetter : concat (replicate 3 timeBlock) ++ [otherSetter]) (otherSetter : timeBlock ++ [otherSetter]))
          == Right (Just ([(1, 1, 3)], [(1, 1, 1)])))
    , check "a block with a different value is not collapsed into the run before it"
        (fmap blockRunsOf (comparedSetters (timeBlock ++ changedBlock) (timeBlock ++ changedBlock)) == Right (Just ([(0, 0, 1), (1, 4, 1)], [(0, 0, 1), (1, 4, 1)]))
          && comparedSetters (timeBlock ++ changedBlock) (timeBlock ++ timeBlock) `refusedAs` SetterSequenceDiffers crawlRoute
          && comparedSetters (timeBlock ++ changedBlock) timeBlock `refusedAs` SetterSequenceDiffers crawlRoute)
    , check "a call between two blocks keeps them as separate runs"
        (fmap blockRunsOf (comparedSetters (timeBlock ++ [otherSetter] ++ timeBlock) (timeBlock ++ [otherSetter] ++ timeBlock))
          == Right (Just ([(0, 0, 1), (2, 5, 1)], [(0, 0, 1), (2, 5, 1)]))
          && comparedSetters (timeBlock ++ [otherSetter] ++ timeBlock) (timeBlock ++ timeBlock ++ [otherSetter]) `refusedAs` SetterSequenceDiffers crawlRoute)
    ]
  unless (and results) exitFailure
  putStrLn ("property-pair-tests: " ++ show (length results) ++ " passed")
