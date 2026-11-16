{-# LANGUAGE OverloadedStrings #-}
module Main (main) where

import qualified A32 as A
import Control.Monad (forM, unless, when)
import Data.Aeson (Value, object, (.=))
import Data.Bits ((.&.))
import qualified Data.ByteString as B
import Data.Word (Word32)
import Emulation.Sha256 (sha256, sha256File, sha256Hex)
import FujiAF.JSON (encodeCanonical)
import qualified FujiAF.UpdateContainer as U
import qualified Replacement.StartupArtifact.Footprint.GeneralHeap as Refs
import StockInventory
import System.Directory (doesPathExist)
import System.Environment (getArgs)

newtype FunctionName = FunctionName String
newtype Digest = Digest String

data Definition = Exact | Nonmatching deriving Show

data Image = Image String Word32 Digest B.ByteString

data Function = Function FunctionName Word32 Word32 Definition

functions :: [Function]
functions = [Function (FunctionName name) start end status | (name,start,end,status) <-
  [("open",0x6841f4,0x684308,Exact),("close",0x684308,0x6843c8,Exact),
   ("program",0x6843c8,0x684514,Exact),("trigger",0x684514,0x684554,Exact),
   ("read-produced",0x684554,0x6845a8,Exact),("poll-done",0x6845a8,0x6845f8,Exact),
   ("check-errors",0x6845f8,0x684638,Exact),("start",0x684638,0x6846c4,Exact),
   ("stop",0x6846c4,0x684760,Exact),("submit",0x684760,0x6847d8,Exact),
   ("kick",0x6847d8,0x684828,Exact),("produced",0x684828,0x684880,Exact),
   ("finished",0x684880,0x6848d8,Exact),("ready-length",0x6ae18c,0x6ae1dc,Exact),
   ("progress",0x6ae1dc,0x6ae310,Nonmatching),("complete",0x6ae310,0x6ae360,Exact),
   ("init",0x6ae360,0x6ae3f4,Exact),("finish",0x6ae3f4,0x6ae500,Nonmatching),
   ("feed",0x6ae500,0x6ae638,Exact)]]

canonicalFirmware :: String
canonicalFirmware = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"

imageSpecs :: [(String,Word32,String)]
imageSpecs =
  [("image4",0x61c400,"f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"),
   ("image5",0xbf1a00,"3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"),
   ("plain",0x1100000,"ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"),
   ("loader-a",0x40000,"21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"),
   ("loader-b",0xd0000,"8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec")]

inside :: Word32 -> Bool
inside address = (address >= 0x6841f4 && address < 0x6848d8) || (address >= 0x6ae18c && address < 0x6ae638)

state :: Word32 -> Bool
state raw = let address = raw .&. 0x3fffffff in address `elem`
  ([0x89f750,0x89f754,0x89f758,0x9c2f54,0x9c2f58] ++ [0x8e1900,0x8e1904..0x8e191c])

reference :: Refs.Reference -> Value
reference value = object ["site" .= Refs.referenceSite value,"kind" .= kind,"target" .= target]
  where (kind,target) = case value of
          Refs.BranchReference _ address -> ("branch" :: String,address)
          Refs.LiteralReference _ address -> ("aligned-word",address)
          Refs.FormedReference _ address -> ("formed-candidate",address)

main :: IO ()
main = do
  args <- getArgs
  (firmware,paths,output) <- case args of
    [dat,a,b,c,d,e,out] -> pure (dat,[a,b,c,d,e],out)
    _ -> fail "usage: image5-decoder-source CANONICAL_DAT IMAGE4 IMAGE5 PLAIN LOADER_A LOADER_B OUTPUT_JSON"
  exists <- doesPathExist output
  when exists (fail "output already exists")
  firmwareDigest <- sha256Hex <$> sha256File firmware
  unless (firmwareDigest == canonicalFirmware) (fail "canonical DAT identity differs")
  images <- forM (zip imageSpecs paths) $ \((name,base,expected),path) -> do
    bytes <- B.readFile path
    let digest = sha256Hex (sha256 bytes)
    unless (digest == expected) (fail (name ++ " identity differs"))
    pure (Image name base (Digest digest) bytes)
  image4 <- case images of Image _ _ _ bytes : _ -> pure bytes; _ -> fail "image4 absent"
  derivations <- forM functions $ \(Function (FunctionName name) start end status) -> do
    inspection <- either (fail . ((name ++ ": ") ++)) pure
      (inspectFunction xm1TaskRecords (GuestAddress 0x61c400) image4 (FunctionWindow (GuestAddress start) (GuestAddress end)) [])
    let calls = [(pc,target) | GuestAddress pc <- inspectedCodeWords inspection,
          A.Branch _ True target <- [A.decode pc (U.word32At image4 (fromIntegral (pc-0x61c400)))]]
        bytes = B.take (fromIntegral (end-start)) (B.drop (fromIntegral (start-0x61c400)) image4)
    pure (object ["name" .= name,"start" .= start,"end" .= end,"definition" .= show status,
      "sha256" .= sha256Hex (sha256 bytes),"code_words" .= length (inspectedCodeWords inspection),
      "literal_words" .= [pc | GuestAddress pc <- inspectedLiteralWords inspection],
      "return_analysis" .= either id (const "derived") (inspectedReturn inspection),
      "calls" .= [object ["pc" .= pc,"target" .= target] | (pc,target) <- calls]])
  let references = [object ["image" .= name,"references" .= map reference selected] |
        Image name base _ bytes <- images,
        let selected = Refs.referencesNaming (\value -> inside value || state value) (Refs.references base bytes)]
      irqWords = [U.word32At image4 (fromIntegral (address-0x61c400)) | address <- ([0x7c95bc,0x7c95c0,0x7c95c4] :: [Word32])]
  unless (irqWords == [69,13,1]) (fail "IRQ source98 table binding differs")
  B.writeFile output (encodeCanonical (object
    ["schema" .= (1 :: Int),"firmware_sha256" .= firmwareDigest,
     "decomp_commit" .= ("55436fc85c0560dd252b0410af6f2427fe198cd3" :: String),
     "images" .= [object ["name" .= name,"base" .= base,"bytes" .= B.length bytes,"sha256" .= digest] | Image name base (Digest digest) bytes <- images],
     "functions" .= derivations,"reference_candidates" .= references,
     "irq" .= object ["source" .= (98 :: Int),"line_priority_mode" .= irqWords,"attach" .= (0x64e918 :: Int),"detach" .= (0x64eb20 :: Int)],
     "publications" .= [object ["address" .= (address :: Word32),"role" .= (role :: String)] |
       (address,role) <- [(0x6ae360,"init"),(0x6ae3f4,"finish"),(0x6ae500,"feed"),(0x6ae1dc,"progress-irq"),(0x6ae310,"final-irq")]],
     "preserved_settings_stub" .= ([0x6ae638,0x6ae650] :: [Word32]),
     "contract" .= (["init saves its task query result, takes resource0x48, reserves engine0x2b/clock41/route and resets the device without polling job completion",
       "feed updates available while active and never overwrites a running job; registered source98 callbacks account actual produced bytes and detach before registering the next handler",
       "final callback gives0x48 only for nonzero saved init-time task, independent of callback-time task identity",
       "finish exposes the declared total, waits for IRQ-driven drain, closes the device and releases engine/clock/route before the final0x48 give",
       "registration and stream semaphore return statuses are ignored by stock; engine acquisition/release errors halt with the reached ownership state retained"] :: [String]),
     "limits" .= (["source/candidate census, not dynamic computed-call closure or hardware observation",
       "formed references are conservative MOVW/MOVT candidates and may include stale-register/data false positives",
       "all aligned input words are scanned; branch candidates in data are not executable-call claims",
       "no producer, callback, IRQ, device law or replacement execution is qualified by this source document"] :: [String])]))
  putStrLn "nineteen decoder function windows and five-image reference/source98 contract written"
