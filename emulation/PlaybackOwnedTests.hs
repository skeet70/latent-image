{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import Control.Monad (unless)
import qualified Crypto.Hash.SHA256 as SHA
import Data.Aeson (Value (..), object, (.=))
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
elf = B.pack [byte i | i <- [0..0x3c7]]
  where
    names = "\0xm1_playback_source_entry\0xm1_playback_mode_entry\0"
    sectionNames = "\0.text\0.symtab\0.strtab\0.shstrtab\0"
    header = B.pack [127,69,76,70,1,1,1] <> B.replicate 9 0
      <> B.pack [2,0,40,0] <> word 1 <> word 0x10000 <> word 0 <> word 0x300 <> word 0
      <> B.pack [52,0,32,0,0,0,40,0,5,0,4,0]
    symbol name value = word name <> word value <> word 4 <> B.pack [0x12,0,1,0]
    section name typ flags addr offset size link entsize = B.concat (map word [name,typ,flags,addr,offset,size,link,0,4,entsize])
    sections = B.replicate 40 0
      <> section 1 1 6 0x10000 0x100 8 0 0
      <> section 7 2 0 0 0x110 48 3 16
      <> section 15 3 0 0 0x140 (fromIntegral (B.length names)) 0 0
      <> section 23 3 0 0 0x200 (fromIntegral (B.length sectionNames)) 0 0
    chunks = [(0,header),(0x100,B.replicate 8 0),(0x110,B.replicate 16 0 <> symbol 1 0x10000 <> symbol 27 0x10004),(0x140,names),(0x200,sectionNames),(0x300,sections)]
    byte i = case [B.index bytes (i-offset) | (offset,bytes) <- chunks, i >= offset, i-offset < B.length bytes] of
      x:_ -> x
      [] -> 0

runtime :: B.ByteString
runtime = B.replicate 256 0

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
  check "guest upload alone has no execution witness" (isRight (P.checkGuestArtifacts package (P.guestArtifacts package)))
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
  check "pair needs actual entries" (P.witness package ack both both == Right (P.Witness True True))
  check "missing reachable mode stays unobserved" (P.witness package ack one one == Right (P.Witness True False))
  let store target = put "source" (String "cpu-write") (put "value" (A.toJSON (1 :: Int)) (put "before" (A.toJSON ([0,0,0,0] :: [Int])) (fetch target)))
  check "publication writes refused" (isLeft (P.witness package ack (state [store 0x120b410]) (state [store 0x120b410])))
  check "original body writes refused" (isLeft (P.witness package ack (state [store 0x120b414]) (state [store 0x120b414])))
  check "owned runtime writes refused" (isLeft (P.witness package ack (state [store 0x10000]) (state [store 0x10000])))
  check "upload is not execution" (P.witness package ack none none == Right (P.Witness False False))
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
  putStrLn "owned playback package and bounded witness behavior passed"
