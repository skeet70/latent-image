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
elfForFamily retained functions = B.pack [byte i | i <- [0..0xfff]]
  where
    encoded = map (B.pack . map (fromIntegral . fromEnum) . T.unpack) functions
    names = B.singleton 0 <> B.concat [name <> B.singleton 0 | name <- encoded]
    nameOffsets = scanl (\offset name -> offset + fromIntegral (B.length name) + 1) 1 encoded
    sectionNames = "\0.text\0.symtab\0.strtab\0.shstrtab\0.ram_push.rendezvous\0.ram_push.mailbox\0.ram_push.publications\0"
    header = B.pack [127,69,76,70,1,1,1] <> B.replicate 9 0
      <> B.pack [2,0,40,0] <> word 1 <> word 0x10000 <> word 0 <> word 0x400 <> word 0
      <> B.pack [52,0,32,0,0,0,40,0,8,0,4,0]
    symbol name index function = case lookup function retainedTargets of
      Just target | retained -> word name <> word target <> word 0 <> B.pack [0x12,0,0xf1,0xff]
      _ -> word name <> word (0x10000 + index * 4) <> word 4 <> B.pack [0x12,0,1,0]
    retainedTargets = [(T.pack name,C.bodyStart body) | service <- C.jpegDeviceServices, let (body,name) = C.jpegDeviceContract service]
    symbols = B.replicate 16 0 <> B.concat [symbol name index function | (name,index,function) <- zip3 (take (length functions) nameOffsets) [0..] functions]
    section name typ flags addr offset size link entsize = B.concat (map word [name,typ,flags,addr,offset,size,link,0,4,entsize])
    sections = B.replicate 40 0
      <> section 1 1 6 0x10000 0x100 (fromIntegral (length functions) * 4) 0 0
      <> section 7 2 0 0 0x800 (fromIntegral (B.length symbols)) 3 16
      <> section 15 3 0 0 0xa00 (fromIntegral (B.length names)) 0 0
      <> section 23 3 0 0 0xe00 (fromIntegral (B.length sectionNames)) 0 0
      <> section 33 1 3 0x15000 0x600 128 0 0
      <> section 54 1 3 0x15080 0x680 128 0 0
      <> section 72 1 3 0x15100 0x700 256 0 0
    chunks = [(0,header),(0x800,symbols),(0xa00,names),(0xe00,sectionNames),(0x400,sections)]
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
        | (entry,index) <- zip C.playbackEntries [0..]
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
    (isLeft (P.parsePackage (json jpegManifest) (json jpegMetadata) runtime (elfFor (allNames ++ take 1 ownedNames)))))
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
  putStrLn "owned playback package and bounded witness behavior passed"
