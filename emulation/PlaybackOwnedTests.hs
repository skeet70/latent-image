{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless, forM_)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (Value (..), object, (.=))
import qualified Data.Aeson.Key as K
import qualified Data.Aeson as A
import qualified Data.Aeson.KeyMap as KM
import Data.Bits (shiftR)
import qualified Data.ByteString as B
import qualified Data.ByteString.Lazy as BL
import Data.Either (isLeft, isRight)
import qualified Data.Text as T
import Data.Word (Word32)
import qualified Emulation.PlaybackOwned as P
import Numeric (showHex)
import qualified PlaybackContract as C

check :: String -> Bool -> IO ()
check name ok = unless ok (fail name)

json :: Value -> B.ByteString
json = BL.toStrict . A.encode

sha :: B.ByteString -> T.Text
sha = T.pack . concatMap (\b -> let s = showHex b "" in replicate (2 - length s) '0' ++ s) . B.unpack . SHA.hash

hex :: Word32 -> T.Text
hex value = T.pack ("0x" ++ showHex value "")

word :: Word32 -> B.ByteString
word value = B.pack [fromIntegral (value `shiftR` n) | n <- [0,8,16,24]]

elf :: B.ByteString
elf = elfFor ["xm1_playback_source_entry", "xm1_playback_mode_entry"]

elfFor :: [T.Text] -> B.ByteString
elfFor = elfForFamily False

elfForFamily :: Bool -> [T.Text] -> B.ByteString
elfForFamily retained functions = B.pack [byte i | i <- [0..fileEnd]]
  where
    lifecycle = "xm1_playback_jpeg_initialize" `elem` functions
    stride = if lifecycle then 8 else 4
    fileEnd = if lifecycle then 0x1fff else 0xfff
    stringsOffset = if lifecycle then 0xc00 else 0xa00
    sectionNamesOffset = if lifecycle then 0x1c00 else 0xe00
    extraSymbols :: [(T.Text,Word32,Word32)]
    extraSymbols = if lifecycle then
      [("xm1_playback_jpeg_finish_released",0x10074,1)
      ,("xm1_playback_jpeg_cancel_released",0x1007c,1)
      ,("__ram_push_publications_start",0x15100,0xfff1)
      ,("__ram_push_publications_end",0x15320,0xfff1)
      ,("__experiment_installer_end",0x15320,0xfff1)] else []
    encoded = map (B.pack . map (fromIntegral . fromEnum) . T.unpack) (functions ++ [name | (name,_,_) <- extraSymbols])
    names = B.singleton 0 <> B.concat [name <> B.singleton 0 | name <- encoded]
    nameOffsets = scanl (\offset name -> offset + fromIntegral (B.length name) + 1) 1 encoded
    sectionNames = "\0.text\0.symtab\0.strtab\0.shstrtab\0.ram_push.rendezvous\0.ram_push.mailbox\0.ram_push.publications\0"
    header = B.pack [127,69,76,70,1,1,1] <> B.replicate 9 0
      <> B.pack [2,0,40,0] <> word 1 <> word 0x10000 <> word 0 <> word 0x400 <> word 0
      <> B.pack [52,0,32,0,0,0,40,0,8,0,4,0]
    symbol name index function = case lookup function retainedTargets of
      Just target | retained -> word name <> word target <> word 0 <> B.pack [0x12,0,0xf1,0xff]
      _ -> word name <> word (0x10000 + index * stride) <> word stride <> B.pack [0x12,0,1,0]
    retainedTargets = [(T.pack name,C.bodyStart body) | service <- C.jpegDeviceServices, let (body,name) = C.jpegDeviceContract service]
    symbols = B.replicate 16 0 <> B.concat [symbol name index function | (name,index,function) <- zip3 (take (length functions) nameOffsets) [0..] functions]
      <> B.concat [word name <> word target <> word 0 <> B.pack [0x10,0,fromIntegral sectionIndex,fromIntegral (sectionIndex `div` 256)]
         | (name,(_,target,sectionIndex)) <- zip (drop (length functions) nameOffsets) extraSymbols]
    section name typ flags addr offset size link entsize = B.concat (map word [name,typ,flags,addr,offset,size,link,0,4,entsize])
    sections = B.replicate 40 0
      <> section 1 1 6 0x10000 0x100 (fromIntegral (length functions) * stride) 0 0
      <> section 7 2 0 0 0x800 (fromIntegral (B.length symbols)) 3 16
      <> section 15 3 0 0 (fromIntegral stringsOffset) (fromIntegral (B.length names)) 0 0
      <> section 23 3 0 0 (fromIntegral sectionNamesOffset) (fromIntegral (B.length sectionNames)) 0 0
      <> section 33 1 3 0x15000 0x600 128 0 0
      <> section 54 1 3 0x15080 0x680 128 0 0
      <> section 72 1 3 0x15100 0x700 (if lifecycle then 544 else 256) 0 0
    chunks = [(0,header),(0x800,symbols),(stringsOffset,names),(sectionNamesOffset,sectionNames),(0x400,sections)]
    byte i = case [B.index bytes (i-offset) | (offset,bytes) <- chunks, i >= offset, i-offset < B.length bytes] of
      x:_ -> x
      [] -> 0

runtime :: B.ByteString
runtime = B.replicate 0x5200 0

completedRuntime :: B.ByteString
completedRuntime = B.take 0x5000 runtime <> B.concat (map word
  [4,9,0,0x4003,0,0,0,0,3,0,0,0,0,0,0,0,9,386,0,0,0,0,0,0,9,0x8003,0,16,0,0,0,0
  ,0x31504d52,7,9,1,0,16,8,8,0,0x4003,9,0x8003,0,16,0x80000,82,9,9,9,291,386,0,0,0,0,0,0,0,0,0,0,0])
  <> B.drop 0x5100 runtime

changeByte :: Int -> B.ByteString -> B.ByteString
changeByte offset bytes = B.take offset bytes <> B.singleton 1 <> B.drop (offset+1) bytes

hook :: Word32 -> Word32 -> Word32 -> T.Text -> T.Text -> Value
hook site end target name hash = object
  ["address" .= hex site,"handler" .= name,"mode" .= ("replace_entry" :: T.Text),"trampoline" .= hex target
  ,"source" .= object ["entry" .= hex site,"end" .= hex end,"sha256" .= hash]
  ,"original_body" .= object ["start" .= hex (site+8),"end" .= hex end]]

metadata :: Value
metadata = object ["schema" .= (1 :: Int),"base" .= hex 0x10000,"runtime_sha256" .= sha runtime
  ,"hooks" .= [hook 0x120b40c 0x120b778 0x10000 "xm1_playback_source_entry" "90da01fffdecd496e473c74aa012c9028be5d2dad33a1b8f4a95b4ea7c466d32"
    ,hook 0x120cab0 0x120cd74 0x10004 "xm1_playback_mode_entry" "d9b5d69e40838f9bcecc142fb1bfeaefa18e43b790a027e0442d7b31deef0f0c"]
  ,"symbols" .= object ["xm1_playback_source_entry" .= hex 0x10000,"xm1_playback_mode_entry" .= hex 0x10004]]

manifest :: Value
manifest = object ["schema" .= (1 :: Int),"base" .= hex 0x10000,"size" .= B.length runtime,"runtime_sha256" .= sha runtime
  ,"publications" .= [object ["address" .= hex site,"old" .= hex 0,"new" .= hex value,"phase" .= phase]
    | (site,value,phase) <- [(0x120b410,0x10000,"setup" :: T.Text),(0x120cab4,0x10004,"setup"),(0x120b40c,0xe51ff004,"activation"),(0x120cab0,0xe51ff004,"activation")]
       ++ [(0x120e000+i*4,0,"setup") | i <- [0..11]]]]

get :: A.Key -> Value -> Value
get key (Object fields) = maybe Null id (KM.lookup key fields)
get _ _ = Null

put :: A.Key -> Value -> Value -> Value
put key value (Object fields) = Object (KM.insert key value fields)
put _ _ value = value

fetch :: Word32 -> Value
fetch target = object ["source" .= ("cpu-fetch" :: T.Text),"address" .= target,"physical" .= target,"size" .= (4 :: Int)
  ,"value" .= Null,"before" .= Null,"byte_order" .= ("little" :: T.Text),"core" .= (0 :: Int),"pc" .= target,"instructions" .= (10 :: Int),"tick" .= (1 :: Int)]

checkJpeg :: IO ()
checkJpeg = do
  let entries = [(start,end,0x10000 + index * 4,T.pack name,T.pack hash)
        | (entry,index) <- zip C.transactionEntries [0..]
        , let (start,end,C.SourceSha256 hash,name) = C.playbackContract entry]
      hooks = [hook start end target name hash | (start,end,target,name,hash) <- entries]
      jpegMetadata = put "hooks" (A.toJSON hooks) (put "symbols" (object [K.fromText name .= hex target | (_,_,target,name,_) <- entries]) metadata)
      jpegManifest = put "publications" (A.toJSON
        ([object ["address" .= hex address,"old" .= hex 0,"new" .= hex value,"phase" .= phase]
          | (start,_,target,_,_) <- entries,(address,value,phase) <- [(start+4,target,"setup" :: T.Text),(start,0xe51ff004,"activation")]]
        ++ [object ["address" .= hex (0x120e000+i*4),"old" .= hex 0,"new" .= hex 0,"phase" .= ("setup" :: T.Text)] | i <- [0..3]])) manifest
      jpegElf = elfFor [name | (_,_,_,name,_) <- entries]
  package <- either fail pure (P.parsePackage (json jpegManifest) (json jpegMetadata) runtime jpegElf)
  check "JPEG scope is separate from preparation pair" (P.qualificationScope package == P.JpegTransaction)
  check "six-hook package retains the device family" (P.deviceOwnership package == P.RetainedJpegDevice)
  let ownedNames = [T.pack name | service <- C.jpegDeviceServices, let (_,name) = C.jpegDeviceContract service]
      allNames = [name | (_,_,_,name,_) <- entries] ++ ownedNames
  owned <- either fail pure (P.parsePackage (json jpegManifest) (json jpegMetadata) runtime (elfFor allNames))
  check "ELF definitions select owned device family" (P.deviceOwnership owned == P.OwnedJpegDevice)
  check "service targets are actual ELF definitions" ([(lo,hi) | (_,lo,hi) <- P.deviceServices (Just owned)]
    == [(0x10018 + index*4,0x1001c + index*4) | index <- [0..9]])
  check "owned family extends source guards by the ten original bodies"
    (drop (length (P.originalBodies package)) (P.originalBodies owned)
      == [(C.bodyStart body,fromIntegral (C.bodyEnd body-C.bodyStart body)) | service <- C.jpegDeviceServices, let (body,_) = C.jpegDeviceContract service])
  forM_ [1..9] $ \count -> check "partial device definition family refused"
    (isLeft (P.parsePackage (json jpegManifest) (json jpegMetadata) runtime (elfFor (take (6+count) allNames))))
  retained <- either fail pure (P.parsePackage (json jpegManifest) (json jpegMetadata) runtime (elfForFamily True allNames))
  check "historical absolute source service family remains admitted" (P.deviceOwnership retained == P.RetainedJpegDevice)
  check "retained source family adds no exclusions" (P.originalBodies retained == P.originalBodies package)
  check "retained service redirected away from pinned source refused"
    (isLeft (P.parsePackage (json jpegManifest) (json jpegMetadata) runtime (changeByte 0x874 (elfForFamily True allNames))))
  check "duplicate device export refused"
    (isLeft (P.parsePackage (json jpegManifest) (json jpegMetadata) runtime (elfFor (allNames ++ take 1 ownedNames))))
  let changedDeviceRuntime = changeByte 0x18 runtime
      changedManifest = put "runtime_sha256" (String (sha changedDeviceRuntime)) jpegManifest
      changedMetadata = put "runtime_sha256" (String (sha changedDeviceRuntime)) jpegMetadata
  check "runtime identity cannot hide ELF device code mismatch"
    (isLeft (P.parsePackage (json changedManifest) (json changedMetadata) changedDeviceRuntime (elfFor allNames)))
  let request = P.watchRequest package
      classes = case (A.fromJSON (get "write_ranges" request), A.fromJSON (get "fetch_ranges" request)) of
        (A.Success writes, A.Success fetches) -> [object ["range" .= range,"kinds" .= kinds]
          | (ranges,kinds) <- [(writes,["cpu-write","host-write","model-bus-write"] :: [T.Text]),(fetches,["cpu-fetch"])], range <- (ranges :: [Value])]
        _ -> []
      ack = object ["armed" .= True,"ranges" .= get "ranges" request,"views" .= ("fixed" :: T.Text),"classes" .= classes]
      state targets = object ["enabled" .= True,"ranges" .= get "ranges" request,"views" .= ("fixed" :: T.Text),"classes" .= classes
        ,"records" .= map fetch targets,"violation" .= Null]
      observed targets = P.witness package ack (state targets) (state targets)
  value <- either fail pure (observed [0x10008,0x10014,0x10010])
  check "JPEG transaction needs its actual lifecycle entries" (P.qualificationObserved package value)
  check "JPEG does not manufacture mode or source entry execution" (not (P.modeObserved value) && not (P.sourceObserved value))
  mapM_ (\targets -> check "incomplete JPEG transaction unqualified" (fmap (P.qualificationObserved package) (observed targets) == Right False))
    [[0x10008,0x10014],[0x10008,0x10010],[0x10014,0x10010],[]]
  mapM_ (\target -> check "retained internal JPEG body execution refused" (isLeft (observed [target])))
    [0xd99714,0xd999d4,0xd99a24]
  let ownedRequest = P.watchRequest owned
      ownedClasses = case (A.fromJSON (get "write_ranges" ownedRequest), A.fromJSON (get "fetch_ranges" ownedRequest)) of
        (A.Success writes,A.Success fetches) -> [object ["range" .= range,"kinds" .= kinds]
          | (ranges,kinds) <- [(writes,["cpu-write","host-write","model-bus-write"] :: [T.Text]),(fetches,["cpu-fetch"])], range <- (ranges :: [Value])]
        _ -> []
      ownedAck = put "classes" (A.toJSON ownedClasses) (put "ranges" (get "ranges" ownedRequest) ack)
      ownedState target = put "classes" (A.toJSON ownedClasses) (put "ranges" (get "ranges" ownedRequest) (state [target]))
  mapM_ (\target -> check "owned device excludes each original low-level body"
    (isLeft (P.witness owned ownedAck (ownedState target) (ownedState target)))) [0x65a478,0x65a58c,0x65a548,0x65ab20,0x65ab50,0x65ab7c,0x65abac,0x65abd8,0x65ac08,0x65a9e4]
  mapM_ (\offset -> check "owned JPEG entry mutation refused" (isLeft (P.checkGuestArtifacts package
    ((0x10000,changeByte offset completedRuntime) : drop 1 (P.guestArtifacts package))))) [8,12,16,20]
  check "partial JPEG ownership refused" (isLeft (P.parsePackage (json jpegManifest) (json (put "hooks" (A.toJSON (take 5 hooks)) jpegMetadata)) runtime jpegElf))

checkConsumers :: IO ()
checkConsumers = do
  let entries = [(start,end,0x10000 + index * 4,T.pack name,T.pack hash)
        | (entry,index) <- zip C.consumerPlaybackEntries [0..]
        , let (start,end,C.SourceSha256 hash,name) = C.playbackContract entry]
      hooks = [hook start end target name hash | (start,end,target,name,hash) <- entries]
      consumerMetadata = put "hooks" (A.toJSON hooks)
        (put "symbols" (object [K.fromText name .= hex target | (_,_,target,name,_) <- entries]) metadata)
      consumerManifest = put "publications" (A.toJSON
        [object ["address" .= hex address,"old" .= hex 0,"new" .= hex value,"phase" .= phase]
        | (start,_,target,_,_) <- entries
        , (address,value,phase) <- [(start+4,target,"setup" :: T.Text),(start,0xe51ff004,"activation")]]) manifest
      names = [name | (_,_,_,name,_) <- entries]
      deviceNames = [T.pack name | service <- C.jpegDeviceServices, let (_,name) = C.jpegDeviceContract service]
      parse meta = P.parsePackage (json consumerManifest) (json meta) runtime (elfFor names)
      changeConsumer key value = put "hooks" (A.toJSON (take 6 hooks ++ [put key value (hooks !! 6)] ++ drop 7 hooks)) consumerMetadata
  package <- either fail pure (parse consumerMetadata)
  owned <- either fail pure (P.parsePackage (json consumerManifest) (json consumerMetadata) runtime (elfFor (names ++ deviceNames)))
  check "consumer scope is explicit" (P.qualificationScope package == P.JpegConsumers)
  check "owned consumers protect twenty-one original extents" (length (P.originalBodies owned) == 21)
  check "consumer source mismatch refused" (isLeft (parse (changeConsumer "source" (put "sha256" (String (T.replicate 64 "0")) (get "source" (hooks !! 6))))))
  check "consumer target mismatch refused" (isLeft (parse (changeConsumer "trampoline" (String (hex 0x10000)))))
  check "consumer site mismatch refused" (isLeft (parse (changeConsumer "address" (String (hex 0xd98908)))))
  check "consumer admission mismatch refused" (isLeft (parse (changeConsumer "mode" (String "observe"))))
  check "seven hooks are not an admitted composition" (isLeft (parse (put "hooks" (A.toJSON (take 7 hooks)) consumerMetadata)))
  let request = P.watchRequest package
      classes = case (A.fromJSON (get "write_ranges" request), A.fromJSON (get "fetch_ranges" request)) of
        (A.Success writes,A.Success fetches) -> [object ["range" .= range,"kinds" .= kinds]
          | (ranges,kinds) <- [(writes,["cpu-write","host-write","model-bus-write"] :: [T.Text]),(fetches,["cpu-fetch"])]
          , range <- (ranges :: [Value])]
        _ -> []
      ack = object ["armed" .= True,"ranges" .= get "ranges" request,"views" .= ("fixed" :: T.Text),"classes" .= classes]
      state targets = object ["enabled" .= True,"ranges" .= get "ranges" request,"views" .= ("fixed" :: T.Text),"classes" .= classes
        ,"records" .= map fetch targets,"violation" .= Null]
      observe targets = P.witness package ack (state targets) (state targets)
      playTargets = [0x10008,0x10014,0x10010,0x1001c]
  prefix <- either fail pure (observe [0x10000,0x10004,0x10018])
  play <- either fail pure (observe playTargets)
  empty <- either fail pure (observe [])
  let evidence = P.qualificationEvidence prefix play
  check "consumers qualify across preparation and PLAY without prefix acquire" (P.qualificationEpochsObserved package evidence)
  check "single PLAY witness cannot qualify consumer scope" (not (P.qualificationObserved package play))
  check "preparation does not manufacture PLAY source or mode" (not (P.sourceObserved (P.playWitness evidence)) && not (P.modeObserved (P.playWitness evidence)))
  check "prefix retains source and mode independently" (P.sourceObserved (P.preparationWitness evidence) && P.modeObserved (P.preparationWitness evidence))
  withoutBuilder <- either fail pure (observe [0x10000,0x10004])
  check "missing preparation builder refuses consumer scope" (not (P.qualificationEpochsObserved package (P.qualificationEvidence withoutBuilder play)))
  forM_ playTargets $ \missing -> do
    partial <- either fail pure (observe (filter (/= missing) playTargets))
    check "each PLAY lifecycle consumer observation is required" (not (P.qualificationEpochsObserved package (P.qualificationEvidence prefix partial)))
  misplaced <- either fail pure (observe (0x10018 : playTargets))
  check "PLAY builder cannot substitute preparation builder" (not (P.qualificationEpochsObserved package (P.qualificationEvidence empty misplaced)))
  withoutCompletion <- either fail pure (observe [0x10008,0x10014,0x10010])
  check "preparation completion cannot substitute PLAY completion" (not (P.qualificationEpochsObserved package (P.qualificationEvidence misplaced withoutCompletion)))
  forM_ [0xd9890c,0x121fed0] $ \target ->
    check "original consumer execution is forbidden" (isLeft (observe [target]))
  forM_ [0x18,0x1c] $ \offset ->
    check "owned consumer entry bytes are immutable" (isLeft (P.checkGuestArtifacts package
      ((0x10000,changeByte offset completedRuntime) : drop 1 (P.guestArtifacts package))))
  check "consumer guard omissions cannot qualify" (isLeft (P.witness package ack
    (put "violation" (String "record limit") (state playTargets)) (state playTargets)))
  check "consumer source read omissions are refused" (isLeft (P.checkOriginalBodies owned []))

checkLifecycle :: IO ()
checkLifecycle = do
  let entries = [(entry,start,end,0x10000 + index*8,T.pack name,T.pack hash)
        | (entry,index) <- zip C.playbackEntries [0..]
        , let (start,end,C.SourceSha256 hash,name) = C.playbackContract entry]
      names = [name | (_,_,_,_,name,_) <- entries]
      hooks = [hook start end target name hash | (_,start,end,target,name,hash) <- entries]
      publications = [(site,0,value,phase)
        | (_,start,_,target,_,_) <- entries
        , (site,value,phase) <- [(start+4,target,0),(start,0xe51ff004,1)]]
      vector = B.concat [B.concat (map word [site,old,new,phase]) | (site,old,new,phase) <- publications]
      bytes = B.replicate 0x5100 0 <> vector <> B.replicate 0xe0 0
      meta = put "runtime_sha256" (String (sha bytes)) $ put "hooks" (A.toJSON hooks) $
        put "symbols" (object [K.fromText name .= hex target | (_,_,_,target,name,_) <- entries]) metadata
      man = put "runtime_sha256" (String (sha bytes)) $ put "size" (A.toJSON (B.length bytes)) $
        put "experiment_layout" (object ["table_offset" .= hex 0x5100,"installer_end_offset" .= hex 0x5320]) $
        put "publications" (A.toJSON [object ["address" .= hex site,"old" .= hex old,"new" .= hex new
          ,"phase" .= (if phase == 0 then "setup" else "activation" :: T.Text)] | (site,old,new,phase) <- publications]) manifest
      parse m metadata' runtime' = P.parsePackage (json m) (json metadata') runtime' (elfFor names)
      patch offset value original = B.take offset original <> word value <> B.drop (offset+4) original
      terminal = foldr (uncurry patch) (B.take 0x5100 completedRuntime <> B.drop 0x5100 bytes)
        [(0x506c,34),(0x5094,34),(0x5098,17),(0x509c,17),(0x50b4,34)]
  package <- either fail pure (parse man meta bytes)
  check "lifecycle scope is explicit" (P.qualificationScope package == P.JpegLifecycle)
  check "full lifecycle publishes exactly thirty-four words" (length (P.guestArtifacts package) == 35)
  check "release observations resolve inside their owned functions"
    (P.releasedObservation package C.PlaybackJpegFinishRequest == Just 0x10074
      && P.releasedObservation package C.PlaybackJpegCancelRequest == Just 0x1007c)
  check "actual lifecycle installer counts admitted" (isRight (P.checkGuestArtifacts package
    ((0x10000,terminal) : drop 1 (P.guestArtifacts package))))
  forM_ [0x506c,0x5094,0x5098,0x509c,0x50b4] $ \offset ->
    check "stale fixed-eight terminal counts refused" (isLeft (P.checkGuestArtifacts package
      ((0x10000,patch offset 8 terminal) : drop 1 (P.guestArtifacts package))))
  forM_ C.lifecycleEntries $ \entry -> do
    let (start,end,_,_) = C.playbackContract entry
    check "every whole lifecycle body is excluded" ((start+8,fromIntegral (end-start-8)) `elem` P.originalBodies package)
  check "interior cancellation observation is not an owned entry"
    (let (start,_,_,_) = C.playbackContract C.PlaybackJpegCancelRequest in start == 0x122026c)
  check "full lifecycle requires linked installer layout" (isLeft (parse (case man of
    Object fields -> Object (KM.delete "experiment_layout" fields); _ -> man) meta bytes))
  let corrupt = changeByte 0x5100 bytes
  check "rehashed publication vector corruption refused" (isLeft (parse
    (put "runtime_sha256" (String (sha corrupt)) man)
    (put "runtime_sha256" (String (sha corrupt)) meta) corrupt))
  forM_ [0..16] $ \index -> do
    let changed = take index hooks ++ [put "original_body" (object ["start" .= hex 0x1220288,"end" .= hex 0x1220294]) (hooks !! index)] ++ drop (index+1) hooks
    check "partial original-body exclusions refused" (isLeft (parse man (put "hooks" (A.toJSON changed) meta) bytes))
  let request = P.watchRequest package
      classes = case (A.fromJSON (get "write_ranges" request), A.fromJSON (get "fetch_ranges" request)) of
        (A.Success writes,A.Success fetches) -> [object ["range" .= range,"kinds" .= kinds]
          | (ranges,kinds) <- [(writes,["cpu-write","host-write","model-bus-write"] :: [T.Text]),(fetches,["cpu-fetch"])]
          , range <- (ranges :: [Value])]
        _ -> []
      ack = object ["armed" .= True,"ranges" .= get "ranges" request,"views" .= ("fixed" :: T.Text),"classes" .= classes]
      state targets = object ["enabled" .= True,"ranges" .= get "ranges" request,"views" .= ("fixed" :: T.Text)
        ,"classes" .= classes,"records" .= map fetch targets,"violation" .= Null]
      observe targets = P.witness package ack (state targets) (state targets)
      playTargets = [0x10010,0x10028,0x10020,0x10038,0x10078]
  prefix <- either fail pure (observe [0x10030])
  play <- either fail pure (observe playTargets)
  check "lifecycle requires natural owned cancellation in PLAY"
    (P.qualificationEpochsObserved package (P.qualificationEvidence prefix play))
  missingCancel <- either fail pure (observe (filter (/= 0x10078) playTargets))
  check "other handbacks cannot substitute the cancel entry"
    (not (P.qualificationEpochsObserved package (P.qualificationEvidence prefix missingCancel)))
  forM_ [start | (start,_) <- P.originalBodies package] $ \start ->
    check "every original lifecycle fetch is refused" (isLeft (observe [start]))
  check "stock cancel interior is observation only and cannot execute under ownership" (isLeft (observe [0x1220288]))
  forM_ [C.preparationEntries,C.transactionEntries] $ \historicalEntries -> do
    let subset = [row | row@(entry,_,_,_,_,_) <- entries, entry `elem` historicalEntries]
        count = length subset
        smallNames = [name | (_,_,_,_,name,_) <- subset]
        smallHooks = [hook start end (0x10000+index*4) name hash
          | ((_,start,end,_,name,hash),index) <- zip subset [0..]]
        smallMeta = put "hooks" (A.toJSON smallHooks) $ put "symbols"
          (object [K.fromText name .= hex (0x10000+index*4) | (name,index) <- zip smallNames [0..]]) metadata
        smallMan = put "publications" (A.toJSON [object ["address" .= hex site,"old" .= hex 0,"new" .= hex new,"phase" .= phase]
          | ((_,start,_,_,_,_),index) <- zip subset [0..]
          , (site,new,phase) <- [(start+4,0x10000+index*4,"setup" :: T.Text),(start,0xe51ff004,"activation")]]) manifest
    small <- either fail pure (P.parsePackage (json smallMan) (json smallMeta) runtime (elfFor smallNames))
    check "preparation and transaction packages use actual counts without spare slots" (length (P.guestArtifacts small) == 1+2*count)

main :: IO ()
main = do
  check "default route does not select an install" (P.selection True Nothing Nothing == Right Nothing)
  check "install alone refused" (isLeft (P.selection True (Just "installed") Nothing))
  check "package alone refused" (isLeft (P.selection True Nothing (Just "package")))
  check "owned shutter refused" (isLeft (P.selection False (Just "installed") (Just "package")))
  selected <- either fail (maybe (fail "selection absent") pure) (P.selection True (Just "/installed") (Just "/package"))
  package <- either fail pure (P.parsePackage (json manifest) (json metadata) runtime elf)
  check "runtime mutation refused" (isLeft (P.parsePackage (json manifest) (json metadata) (B.cons 1 (B.drop 1 runtime)) elf))
  check "extra hook refused" (isLeft (P.parsePackage (json manifest) (json (put "hooks" (A.toJSON ([] :: [Value])) metadata)) runtime elf))
  let installed bytes = (0x10000,bytes) : drop 1 (P.guestArtifacts package)
  check "uploaded initializer is not a completed install" (isLeft (P.checkGuestArtifacts package (P.guestArtifacts package)))
  check "completed scalar installation reports admitted" (isRight (P.checkGuestArtifacts package (installed completedRuntime)))
  check "active entry mutation refused" (isLeft (P.checkGuestArtifacts package (installed (changeByte 0 completedRuntime))))
  check "writable publication table mutation refused" (isLeft (P.checkGuestArtifacts package (installed (changeByte 0x5100 completedRuntime))))
  check "rendezvous padding mutation refused" (isLeft (P.checkGuestArtifacts package (installed (changeByte 0x5010 completedRuntime))))
  check "unsuccessful installer status refused" (isLeft (P.checkGuestArtifacts package (installed (changeByte 0x5090 completedRuntime))))
  check "incomplete peer callback refused" (isLeft (P.checkGuestArtifacts package (installed (changeByte 0x50c8 completedRuntime))))
  check "guest publication mutation refused" (isLeft (P.checkGuestArtifacts package [(0x10000,runtime)]))
  check "original source identity mutation refused" (isLeft (P.checkOriginalBodies package [B.replicate 868 0,B.replicate 700 0]))
  let request = P.watchRequest package
      writeRanges = get "write_ranges" request
      fetchRanges = get "fetch_ranges" request
      classes = case (A.fromJSON writeRanges, A.fromJSON fetchRanges) of
        (A.Success writes, A.Success fetches) -> [object ["range" .= range,"kinds" .= kinds] | (ranges,kinds) <- [(writes,["cpu-write","host-write","model-bus-write"] :: [T.Text]),(fetches,["cpu-fetch"])], range <- (ranges :: [Value])]
        _ -> []
      ack = object ["armed" .= True,"ranges" .= get "ranges" request,"views" .= ("fixed" :: T.Text),"classes" .= classes]
      state rows = object ["enabled" .= True,"ranges" .= get "ranges" request,"views" .= ("fixed" :: T.Text),"classes" .= classes,"records" .= rows,"violation" .= Null]
      both = state [fetch 0x10000,fetch 0x10004]
      one = state [fetch 0x10000]
      none = state ([] :: [Value])
  check "watch armed before PLAY" (isRight (P.validateWatchBegin package ack))
  check "unarmed watch refused before PLAY" (isLeft (P.validateWatchBegin package (put "armed" (Bool False) ack)))
  check "pair needs actual entries" (fmap (\w -> (P.sourceObserved w, P.modeObserved w)) (P.witness package ack both both) == Right (True, True))
  check "missing reachable mode stays unobserved" (fmap (\w -> (P.sourceObserved w, P.modeObserved w)) (P.witness package ack one one) == Right (True, False))
  let store target = put "source" (String "cpu-write") (put "value" (A.toJSON (1 :: Int)) (put "before" (A.toJSON ([0,0,0,0] :: [Int])) (fetch target)))
  check "publication writes refused" (isLeft (P.witness package ack (state [store 0x120b410]) (state [store 0x120b410])))
  check "original body writes refused" (isLeft (P.witness package ack (state [store 0x120b414]) (state [store 0x120b414])))
  check "owned runtime writes refused" (isLeft (P.witness package ack (state [store 0x10000]) (state [store 0x10000])))
  check "upload is not execution" (fmap (\w -> (P.sourceObserved w, P.modeObserved w)) (P.witness package ack none none) == Right (False, False))
  check "original source body refused" (isLeft (P.witness package ack (state [fetch 0x120b414]) (state [fetch 0x120b414])))
  check "original mode body refused" (isLeft (P.witness package ack (state [fetch 0x120cab8]) (state [fetch 0x120cab8])))
  check "lost watch rows refused" (isLeft (P.witness package ack (put "violation" (String "record limit") both) both))
  check "changed watch interval refused" (isLeft (P.witness package ack one both))
  let record = object ["schema" .= (1 :: Int),"key" .= ("boot" :: T.Text),"base" .= hex 0x10000,"manifest_sha256" .= sha (json manifest),"runtime_sha256" .= sha runtime,"host" .= object []]
      candidate = put "directory" (String "/installed") (put "record_sha256" (String (sha (json record))) (put "snapshot_sha256" (String (T.replicate 64 "a")) record))
      options = object ["boot_key" .= ("boot" :: T.Text),"installed" .= [candidate]]
  check "matching installed package selected" (isRight (P.validateInstalled selected package (json record) options))
  check "different installed identity refused" (isLeft (P.validateInstalled selected package (json (put "key" (String "other") record)) options))
  check "unlisted install refused" (isLeft (P.validateInstalled selected package (json record) (put "installed" (A.toJSON ([] :: [Value])) options)))
  start <- either fail pure (P.validateInstalled selected package (json record) options)
  let ready = object ["checkpoint" .= object ["start" .= ("installed" :: T.Text),"installed" .= ("/installed" :: T.Text),"snapshot_sha256" .= T.replicate 64 "a"]]
  check "matching installed restore admitted" (isRight (P.validateReady selected start ready))
  check "changed snapshot refused" (isLeft (P.validateReady selected start (put "checkpoint" (put "snapshot_sha256" (String (T.replicate 64 "b")) (get "checkpoint" ready)) ready)))
  check "cold replay refused" (isLeft (P.validateReady selected start (object ["checkpoint" .= object ["start" .= ("cold" :: T.Text)]])))
  checkJpeg
  checkConsumers
  checkLifecycle
  putStrLn "owned playback package and bounded witness behavior passed"
