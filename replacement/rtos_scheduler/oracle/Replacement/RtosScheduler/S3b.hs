{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (forM, unless)
import Data.Aeson (eitherDecodeStrict', object, (.=))
import Data.Bits ((.&.), xor)
import Data.List (isPrefixOf, sortOn)
import qualified Data.ByteString as B
import qualified Data.Map.Strict as M
import Data.Word (Word32, Word8)
import Data.Maybe (catMaybes, fromMaybe, isJust)
import Emulation.NativeObservation
import qualified Emulation.NativeCase as C
import Emulation.OwnedCase
import Emulation.Sha256 (sha256File, sha256Hex)
import FujiAF.Host (get, s, set, str)
import qualified Replacement.RtosPlatform.Aggregate as Agg
import System.Directory (createDirectoryIfMissing, doesFileExist, removeDirectoryRecursive)
import System.Environment (getArgs)
import System.FilePath ((</>))
import FujiAF.JSON (encodeCanonical)

data Service = Activate | InterruptActivate | CancelActivation | Wake | InterruptWake | CancelWake deriving (Eq, Show, Enum, Bounded)
data Row = Row Word32 Word32 Word32 Word8 Word8 Bool Bool Bool Word32 deriving Show

entry :: Service -> Word32
entry s = case s of Activate -> 0x44f2c; InterruptActivate -> 0x450f0; CancelActivation -> 0x45270; Wake -> 0x46004; InterruptWake -> 0x46200; CancelWake -> 0x46398
extent :: Service -> Int
extent s = case s of Activate -> 452; InterruptActivate -> 384; CancelActivation -> 384; Wake -> 508; InterruptWake -> 408; CancelWake -> 412
handler :: Service -> Bool
handler s = s `elem` [InterruptActivate, InterruptWake]
rows :: Service -> [Row]
rows s = [Row h c 0x53 st fl exists discarded self dispatch | h <- [0,1,2,120,0x10001,0x20001,0x10000,0x20000,0xffff,0x30001,121], (st,fl) <- [(0,0),(1,0),(1,1),(1,2),(1,7),(1,0xff),(1,0xf9),(4,0),(0x0a,0),(0x0e,2)], (exists,discarded,self,dispatch) <- [(True,False,False,0),(True,False,False,1),(True,False,False,2),(True,False,False,0x80000000),(True,True,True,1),(False,False,False,1)], let c = if handler s then 1 else 0]
  ++ [Row 0x10001 c mask 1 0 True False False 1 | c <- [0,1], mask <- [0x53,0xd3]]

expected :: Bool -> Service -> Row -> (RunnerStatus, Word32)
expected domainB s (Row h c mask st fl exists discarded self dispatch)
  | (c /= 0) /= handler s || mask .&. 0x80 /= 0 = result (-25)
  | cls > 2 || number > 120 || number >= 0x8000 || (handler s && number == 0) = result (-18)
  | not exists = result (-42)
  | s == CancelActivation = if abandoned then result (-41) else result (fromIntegral (flags .&. 1))
  | s == CancelWake = if state == 0 || abandoned then result (-41) else result (if flags .&. 2 /= 0 then 1 else 0)
  | wake && (state == 0 || (not (handler s) && abandoned)) = result (-41)
  | (not wake && state == 0) || (wake && state .&. 8 /= 0) =
      if not (handler s) && local && dispatch /= 0 && (not wake || state .&. 4 == 0)
      then (StopAddress, fromIntegral (if wake then (-19 :: Int) else -7))
      else result 0
  | flags .&. (if wake then 2 else 1) /= 0 = result (-43)
  | otherwise = result 0
 where
  result n = (Returned, fromIntegral (n :: Int))
  cls = h `div` 0x10000
  number = h .&. 0xffff
  local = cls == 0 || cls == (if domainB then 2 else 1)
  selectedSelf = self && local
  state = if number == 0 && not selectedSelf then 1 else st
  flags = if number == 0 && not selectedSelf then 0 else fl
  abandoned = selectedSelf && discarded
  wake = s `elem` [Wake,InterruptWake]

poke :: B.ByteString -> [(Word32, B.ByteString)] -> B.ByteString
poke = foldl (\bytes (offset,value) -> B.take (fromIntegral offset) bytes <> value <> B.drop (fromIntegral offset + B.length value) bytes)
w :: Word32 -> B.ByteString
w = word32le

spec :: Bool -> Service -> Row -> String -> [(Word32, Word32)] -> [(Word32, Word32)] -> B.ByteString -> CaseSpec
spec domainB service (Row handle context cpsr state flags exists discarded self dispatch) label codeSpans functionSpans code = CaseSpec
  { caseId=label, caseProvenance="X-M1 1.50 exact task_services.c 55436fc85c, S3b", caseExclusion="Stack scratch and live dispatcher frame excluded; retained helpers execute stock", caseEntry=ga (entry service + delta), caseExit=ga 0x958f00
  , caseRegions=codeRegions ++ [mkRegion (RegionId "config") (ga 0x96c000) 0x1000 ReadWrite config, mkRegion (RegionId "stack") (ga 0x959700) 0x110 ReadWrite (B.replicate 0x110 0)] ++ concatMap ram [False,True] ++ [mkRegion (RegionId name) (ga at) 4 Device (w 0) | (name,at) <- [("bank0",0xfffb30cc),("bank1",0xfffb40cc)]]
      ++ [mkRegion (RegionId "code-exit") (ga 0x958f00) 4 ReadExecute (w 0xe7f000f0)]
  , caseFunctions=[(ga at,size) | (at,size) <- functionSpans]
  , caseInitial=[(r,if r==R0 then handle else 0xa5a50000 + fromIntegral (fromEnum r)) | r <- [R0 .. R12]] ++ [(Sp,0x959800),(Lr,0x958f00),(Cpsr,cpsr)]
  , caseExpectedStatus=fst expectation, caseExpectedRegisters=[(R0,snd expectation)], caseExpectedMemory=[], caseCp14Identity=Nothing
  , caseKernel=Just (KernelCase (InstructionLimit 4000) (Just (ga (if domainB then 0xd3640 else 0x43640))) Nothing Nothing Nothing [R1,R2,R3,R12,Lr,Pc,Cpsr] []) }
 where
  expectation = expected domainB service (Row handle context cpsr state flags exists discarded self dispatch)
  ga = GuestAddress . fromIntegral
  delta = if domainB then 0x8ffc0 else 0
  base = if domainB then 0xd0000 else 0x40000
  codeRegions = [mkRegion (RegionId ("code-"++show at)) (ga at) (fromIntegral size) ReadExecute (B.take (fromIntegral size) (B.drop (fromIntegral (at-base)) code)) | (at,size) <- codeSpans]
  localRam = if domainB then 0x400e0000 else 0x40050000
  table = localRam + if domainB then 0x458c else 0x45cc
  own = if domainB then 2 else 1
  targetClass = if handle `div` 0x10000 == 0 then own else handle `div` 0x10000
  targetB = targetClass == 2
  taskAt b = if b then 0x400efecc else 0x4005ff0c
  cfg = 0x96c000
  config = poke (B.replicate 0x1000 0) [(0,w 0),(0x18,w 0x900000),(0x1c,w 0x1000),(0x24,w (if exists then 1 else 0))]
  ram b =
    [mkRegion (RegionId (if b then "globals-b" else "globals-a")) (ga (rb+0x4000)) 0x1000 ReadWrite (B.take 0x1000 (B.drop 0x4000 bytes)),
     mkRegion (RegionId (if b then "tasks-b" else "tasks-a")) (ga (taskAt b)) 0x1a40 ReadWrite (B.take 0x1a40 (B.drop (fromIntegral (taskAt b-rb)) bytes))]
   where
    rb = if b then 0x400e0000 else 0x40050000
    adj = if b then 0x40 else 0
    heads = rb + 0x46dc-adj
    running = rb+0x4760-adj
    selected = rb+0x4768-adj
    number = handle .&. 0xffff
    task = taskAt b + if number >= 1 && number <= 120 then (number-1)*0x38 else 0
    selectedTask = taskAt b + 118*0x38
    selectedDefinition = [(selectedTask+8,w cfg),(selectedTask+0xc,B.singleton 1)]
    local = rb == localRam
    rel at val = (at-rb,val)
    record cls = table + (cls-1)*0x84
    recordFields cls = let other=cls==2; r=record cls; rbase=if other then 0x400e0000 else 0x40050000; a=if other then 0x40 else 0 in [(r,w (if cls==own then 0 else cls)),(r+8,w (rbase+0x45c0-a)),(r+0xc,w (if domainB then 0xd398c else 0x439d8)),(r+0x10,w (if domainB then 0xd39c8 else 0x43a14)),(r+0x20,w (rbase+0x46dc-a)),(r+0x24,w (rbase+0x475c-a)),(r+0x28,w (rbase+0x4760-a)),(r+0x2c,w (rbase+0x4768-a)),(r+0x30,w (rbase+0x4764-a)),(r+0x44,w (taskAt other)),(r+0x48,w 120),(r+0x7c,w (if other then 0xd3728 else 0x43728))]
    seed = poke (B.replicate 0x12000 0) (map (uncurry rel) ([(heads+fromIntegral p*8,w (heads+fromIntegral p*8)) | p <- [0::Int ..15]] ++ [(heads+fromIntegral p*8+4,w (heads+fromIntegral p*8)) | p <- [0::Int ..15]] ++ selectedDefinition ++ [(running,w (if self && b==targetB then task else selectedTask)),(selected,w selectedTask),(rb+0x4764-adj,w dispatch),(task+8,w cfg),(task+0xc,B.pack [state,0,flags,0]),(task+0x14,w (cfg+0x100)),(task+0x30,w (if discarded then 1 else 0)),(selectedTask+0xd,B.singleton 15)] ++ if local then [(rb+0x45a4-adj,w context),(rb+0x45c8-adj,w (record own))] ++ concatMap recordFields [1,2] else []))
    bytes = poke seed (map (uncurry rel) queueFields)
    queueFields =
      [(heads+120,w selectedTask),(heads+124,w selectedTask),
       (selectedTask,w (heads+120)),(selectedTask+4,w (heads+120)),
       (rb+0x475c-adj,w (if state .&. 1 /= 0 then 0x8001 else 0x8000)),
       (selected,w (if state .&. 1 /= 0 then task else selectedTask))]
      ++ [(at,w value) | state .&. 1 /= 0, (at,value) <- [(heads,task),(heads+4,task),(task,heads),(task+4,heads)]]

comparable :: CaseSpec -> NativeObservation -> NativeObservation -> Bool
comparable spec a b = nativeStatus a == nativeStatus b && memoryHolds && all same registers && mode a == mode b && visibleWrites a == visibleWrites b
 where
  sp = fromMaybe 0 (lookup Sp (caseInitial spec))
  registers = [R0] ++ (if nativeStatus a==Returned then [R4 .. R11]++[Sp] else [])
  same r = M.lookup r (nativeRegisters a)==M.lookup r (nativeRegisters b)
  mode observation = (.&. 0x1ff) <$> M.lookup Cpsr (nativeRegisters observation)
  writable permission = case permission of ReadWrite -> True; Device -> True; _ -> False
  stackLow = 0x959700
  memoryHolds = all regionHolds (caseRegions spec)
  regionHolds (CaseRegion raw permission)
    | not (writable permission) = True
    | otherwise = case (M.lookup ident (M.fromList (nativeMemory a)), M.lookup ident (M.fromList (nativeMemory b))) of
        (Just x, Just y) -> liveBytes base x == liveBytes base y
        _ -> False
    where
      ident = regionIdOf raw
      base = fromIntegral (C.baseAddress raw)
  liveBytes base bytes
    | base == stackLow = B.drop (fromIntegral (min (fromIntegral (B.length bytes)) (sp - stackLow))) bytes
    | otherwise = bytes
  visibleWrites observation = [(fromIntegral at, fromIntegral size, value) | Just kernel <- [nativeKernel observation], Just order <- [observedOrder kernel], OrderedWrite (Write (GuestAddress at) (WriteBytes size)) value <- order, deviceAt (fromIntegral at) (fromIntegral size)]
  deviceAt at size = any regionDevice (caseRegions spec)
   where
    regionDevice (CaseRegion raw permission) =
      let base = fromIntegral (C.baseAddress raw)
          top = base + fromIntegral (B.length (C.regionBytes raw))
       in case permission of Device -> base <= at && at + size <= top; _ -> False

runCases :: FilePath -> Maybe FilePath -> FilePath -> [(String, CaseSpec)] -> IO (M.Map String NativeObservation, Int)
runCases runner reuseDirectory directory cases = do
  runnerDigest <- sha256Hex <$> sha256File runner
  results <- forM (zip [0 :: Int ..] (chunks cases)) $ \(batch, labelled) -> do
    let batchName = "batch-" ++ show batch
        currentDirectory = directory </> batchName
    path <- writeBatch currentDirectory (map snd labelled)
    reused <- fmap catMaybes $ forM labelled $ \(label, _) ->
      case reuseDirectory of
        Nothing -> pure Nothing
        Just savedDirectory -> do
          let savedPath = savedDirectory </> batchName </> (label ++ ".native-result.json")
          exists <- doesFileExist savedPath
          if not exists then pure Nothing else do
            saved <- either fail pure . eitherDecodeStrict' =<< B.readFile savedPath
            (current, regions, _, identity) <- C.loadCase (currentDirectory </> (label ++ ".case.json"))
            if str (get saved "runner_sha256") /= runnerDigest || Just (get saved "wire_sha256") /= lookup "wire_sha256" identity
              then pure Nothing
              else do
                exit <- either (fail . show) pure (reportRunnerExit saved)
                let report = set "runner_sha256" (s runnerDigest) (C.caseReport current identity)
                (_, judged) <- C.judgeRunner current regions report exit (encodeCanonical (get saved "runner")) B.empty
                B.writeFile (currentDirectory </> (label ++ ".native-result.json")) (encodeCanonical judged)
                observation <- either (fail . show) pure (observeNative exit regions (get judged "runner"))
                pure (Just (CaseId label, observation))
    let reusedLabels = M.fromList [(label, ()) | (CaseId label, _) <- reused]
        freshSpecs = [specification | (label, specification) <- labelled, M.notMember label reusedLabels]
    fresh <- if null freshSpecs then pure [] else loadObservedBatch runner =<< writeBatch (currentDirectory </> "fresh") freshSpecs
    let observed = reused ++ fresh
    unless (length observed == length labelled) (fail (path ++ ": observation count differs"))
    pure ([(label, observation {nativeMemory = [(ident, B.copy bytes) | (ident, bytes) <- nativeMemory observation, not ("code-" `isPrefixOf` regionName ident)]}) | (CaseId label, observation) <- observed], length reused)
  pure (M.fromList (concatMap fst results), sum (map snd results))
 where
  chunks [] = []
  chunks items = let (batch, rest) = splitAt 4096 items in batch : chunks rest

main :: IO ()
main = do
  arguments <- getArgs
  let positional = stripSymbolFlags arguments
      symbolsA = flagValue "--symbols-a" arguments
      symbolsB = flagValue "--symbols-b" arguments
      placementsA = flagValue "--placements-a" arguments
      placementsB = flagValue "--placements-b" arguments
      reuseRoot = flagValue "--reuse-root" arguments
  case positional of
    [runner,loaderA,loaderB,binA,binB,work] -> do
      createDirectoryIfMissing True work
      reports <- forM [(False,loaderA,binA,symbolsA,placementsA),(True,loaderB,binB,symbolsB,placementsB)] $ \(b,loaderPath,binPath,symbolsPath,placementsPath) -> do
        stock <- B.readFile loaderPath
        owned <- B.readFile binPath
        symbols <- traverse readSymbols symbolsPath
        placements <- traverse readPlacements placementsPath
        unless (isJust symbolsPath == isJust placementsPath) (fail "aggregate symbols and placements must be supplied together")
        aggregateImage <- traverse (\rows -> either fail pure (Agg.aggregateImage rows owned)) symbols
        loaderDigest <- sha256Hex <$> sha256File loaderPath
        unless (loaderDigest == if b then "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec" else "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882") (fail "loader is not pinned X-M1 1.50")
        candidateDigest <- sha256Hex <$> sha256File binPath
        runnerDigest <- sha256Hex <$> sha256File runner
        symbolDigest <- traverse (fmap sha256Hex . sha256File) symbolsPath
        placementDigest <- traverse (fmap sha256Hex . sha256File) placementsPath
        B.writeFile (work </> ("inputs-" ++ show b ++ ".json")) (encodeCanonical (object ["loader_sha256" .= loaderDigest, "candidate_sha256" .= candidateDigest, "runner_sha256" .= runnerDigest, "symbols_sha256" .= symbolDigest, "placements_sha256" .= placementDigest]))
        let delta=if b then 0x8ffc0 else 0; base=if b then 0xd0000 else 0x40000
            selected = fromMaybe [] symbols
            patch s bytes = poke bytes [(entry s+delta-base,B.take (extent s) (B.drop (fromIntegral (entry s-entry Activate)) owned))]
            candidate = case aggregateImage of
              Nothing -> foldr patch stock [minBound..maxBound]
              Just image ->
                let withSymbols = foldl (\bytes symbol -> either error id (Agg.installSymbol base bytes image symbol)) stock selected
                 in case placements of
                      Just plans -> either error id (Agg.installPlacements base withSymbols image plans)
                      Nothing -> withSymbols
            symbolSpans = [(Agg.symbolAddress symbol, Agg.symbolSize symbol) | symbol <- selected]
            placementSpans = maybe [] (map (\placement -> (Agg.placementAddress placement, Agg.placementBytes placement))) placements
            helperSpans = helperFunctions b
            helperBase = if b then 0xd33e4 else 0x433e4
            helperSize = if b then 0x1210 else 0x1250
            stockCodeSpans s = [(entry s+delta, fromIntegral (extent s)), (helperBase, helperSize)]
            stockFunctionSpans s = (entry s+delta, fromIntegral (extent s)) : helperSpans
            aggregateLo = minimum (map fst (symbolSpans ++ helperSpans ++ placementSpans))
            aggregateHi = maximum [address + size | (address, size) <- symbolSpans ++ helperSpans ++ placementSpans]
            ownedCodeSpans s = case symbols of
              Nothing -> stockCodeSpans s
              Just _ -> [(aggregateLo, aggregateHi-aggregateLo)]
            placementFunctions = maybe [] (map (\placement -> (Agg.placementAddress placement, Agg.placementBytes placement)) . filter (\placement -> ".text." `isPrefixOf` Agg.placementSection placement)) placements
            ownedFunctionSpans s = case symbols of
              Nothing -> stockFunctionSpans s
              Just _ -> mergeSpans (symbolSpans ++ helperSpans ++ placementFunctions)
        forM [minBound..maxBound] $ \s -> do
          let cases=rows s
              scratchInputs specification = specification {caseInitial = [(register, if register `elem` [R1, R2, R3, R12] then value `xor` 0xffffffff else value) | (register, value) <- caseInitial specification]}
              roles=[("stock",stock,stockCodeSpans s,stockFunctionSpans s,id),("owned",candidate,ownedCodeSpans s,ownedFunctionSpans s,id),("equivalent",candidate,ownedCodeSpans s,ownedFunctionSpans s,scratchInputs),("mutant",poke candidate [(entry s+delta-base,w 0xe3e00010<>w 0xe12fff1e)],ownedCodeSpans s,ownedFunctionSpans s,id)]
              labelled=[(role++show i,alter (spec b s row (role++show i) codeSpans functionSpans bytes)) | (role,bytes,codeSpans,functionSpans,alter)<-roles,(i,row)<-zip [0::Int ..] cases]
              dir=work </> (show b++show s)
          (observed, reused) <- runCases runner (fmap (</> (show b ++ show s)) reuseRoot) dir labelled
          let at role i=observed M.! (role++show i)
              completed=and [nativeStatus (at "stock" i) == status && M.lookup R0 (nativeRegisters (at "stock" i)) == Just value | (i,row) <- zip [0 :: Int ..] cases, let (status,value)=expected b s row]
              stockSpec i row=spec b s row ("stock"++show i) (stockCodeSpans s) (stockFunctionSpans s) stock
              matches role=length [() | i<-[0..length cases-1],comparable (stockSpec i (cases!!i)) (at "stock" i) (at role i)]
              corruptByte name offset observation = observation { nativeMemory = [(ident, if regionName ident == name then B.take offset bytes <> B.singleton (B.index bytes offset + 1) <> B.drop (offset + 1) bytes else bytes) | (ident, bytes) <- nativeMemory observation] }
              controls = case cases of
                [] -> []
                row : _ ->
                  let reference = at "stock" 0
                      fixture = stockSpec 0 row
                      refuses change = not (comparable fixture reference (change reference))
                   in [refuses (corruptByte "tasks-a" 0x0c), refuses (corruptByte "globals-a" 0), refuses (corruptByte "stack" 0x100), comparable fixture reference (corruptByte "stack" 0 reference)]
              negativeControl = length controls == 4 && and controls
              holds=completed && matches "owned"==length cases && matches "equivalent"==length cases && matches "mutant"<length cases && negativeControl
          unless holds $ putStrLn (show (b,s,completed,matches "owned",length cases,[(i,nativeStatus (at "stock" i),nativeStatus (at "owned" i)) | i<-[0..length cases-1],not (comparable (stockSpec i (cases!!i)) (at "stock" i) (at "owned" i))]))
          if holds then removeDirectoryRecursive dir else pure ()
          let report=object ["domain_b".=b,"service".=show s,"cases".=length cases,"reused_observations".=reused,"stock_completed".=completed,"owned_matches".=matches "owned","equivalent_matches".=matches "equivalent","mutant_matches".=matches "mutant","negative_control".=negativeControl,"passed".=holds]
          B.writeFile (work </> (show b++show s++".json")) (encodeCanonical report)
          pure (holds,report)
      let flat=concat reports
      B.writeFile (work </> "summary.json") (encodeCanonical (object ["passed".=all fst flat,"entries".=map snd flat]))
      unless (all fst flat) (fail "S3b oracle failed")
    _ -> fail "runner loader-a loader-b owned-a owned-b work"

helperFunctions :: Bool -> [(Word32, Word32)]
helperFunctions domainB = if domainB then [(0xd3f2c,44),(0xd3c60,124),(0xd4528,204),(0xd3d8c,72),(0xd3fd4,48),(0xd33e4,96),(0xd398c,60),(0xd39c8,40)] else [(0x43f70,44),(0x43ca4,124),(0x44568,204),(0x43dd0,72),(0x44018,44),(0x433e4,96),(0x439d8,60),(0x43a14,40)]

mergeSpans :: [(Word32, Word32)] -> [(Word32, Word32)]
mergeSpans spans = go (sortOn fst spans)
  where
    go []=[]
    go ((start,size):rest)=extend start (start+size) rest
    extend start end []=[(start,end-start)]
    extend start end ((lo,size):rest)
      | lo<end=extend start (max end (lo+size)) rest
      | otherwise=(start,end-start):extend lo (lo+size) rest

readSymbols :: FilePath -> IO [Agg.AggregateSymbol]
readSymbols path = either fail pure . Agg.parseAggregateSymbols =<< B.readFile path

readPlacements :: FilePath -> IO [Agg.SectionPlacement]
readPlacements path = either fail pure . Agg.parsePlacements =<< B.readFile path

flagValue :: String -> [String] -> Maybe FilePath
flagValue flag arguments = case dropWhile (/= flag) arguments of
  [] -> Nothing
  [_] -> Nothing
  _ : value : _ -> Just value

stripSymbolFlags :: [String] -> [String]
stripSymbolFlags (flag : _ : rest) | flag `elem` ["--symbols-a", "--symbols-b", "--placements-a", "--placements-b", "--reuse-root"] = stripSymbolFlags rest
stripSymbolFlags (value : rest) = value : stripSymbolFlags rest
stripSymbolFlags [] = []
