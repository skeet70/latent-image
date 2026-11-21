{-# LANGUAGE GHC2021 #-}

-- | NUL-terminated string objects whose aligned words the property scan reads as values in retired
-- property code. Each container, every reader its handles reach, and the handle census over the
-- scanned images are derived from the images at call time.
module Replacement.PropertyService.RetainedStrings
  ( stringClosures
  ) where

import Data.List (sort)
import qualified Data.Set as S
import Data.Word (Word32)
import qualified Emulation.Sha256 as SHA
import Replacement.PropertyService.Placement (Carrier (..), GuestAddress (..), Reference (..), ScanHit (..), ScanImage (..), hitTarget, scanReferences)
import Replacement.PropertyService.RetainedClosure
import ServiceInventory (Image, imageWord)

data StringRole
  = DpsContinueJobElement
  | CalcsubTaskName
  | PlaysubTaskName
  | UsbTaskName
  | AssocResponseLogFormat
  | ActionLogFormat
  | HwMode80211bText
  deriving (Eq, Show)

-- | Bytes [start, end) of one image, bound by digest.
data Extent = Extent ScanImage Word32 Word32 SHA.Sha256

-- | A movw at this address whose movt completes an address in a census extent.
data Handle = MovwMovt ScanImage Word32
  deriving (Eq, Ord, Show)

-- | The census of an extent finds exactly these handles and no other reference.
data Census = Census Extent [Handle]

data StringSite = StringSite
  { siteRole :: StringRole
  , siteImage :: ScanImage
  , siteAddress :: Word32
  , siteRoleClosure :: ClosedRole
  , siteCensus :: [Census]
  , siteReaders :: [Extent]
  }

stringClosures :: [(ScanImage, Image)] -> [ClosedSite]
stringClosures images
  | not (all (\label -> any ((== label) . fst) images) scannedImages) = []
  | otherwise =
      [ ClosedSite (siteImage site) (GuestAddress (siteAddress site)) word (siteRoleClosure site)
      | site <- stringSites
      , containsSite site
      , all (extentBound images) (siteReaders site)
      , all censusHolds (siteCensus site)
      , Just image <- [imageFor (siteImage site) images]
      , Just word <- [imageWord image (siteAddress site)]
      , word == roleWord (siteRole site)
      ]
  where
    found = filter (\hit -> any (inExtent (hitTarget hit)) censusExtents) (scanReferences censusHull images S.empty)
    censusExtents = [bound | site <- stringSites, Census bound _ <- siteCensus site]
    censusHolds (Census bound expected) =
      extentBound images bound
        && maybe False ((== sort expected) . sort) (traverse handle [hit | hit <- found, inExtent (hitTarget hit) bound])
    handle hit = case hitReference hit of
      MovPairInto _ -> Just (MovwMovt (hitImage hit) (unGuest (hitSite hit)))
      _ -> Nothing
    unGuest (GuestAddress address) = address

inExtent :: GuestAddress -> Extent -> Bool
inExtent (GuestAddress address) (Extent _ start end _) = address >= start && address < end

-- | Spans every census extent; the stager's reference scan into it is filtered to the extents.
-- Base+index arithmetic without a materialized base is not covered.
censusHull :: Carrier
censusHull = Carrier (GuestAddress 0xd80dd4) (GuestAddress 0x13a2814)

-- | The first census extent holds the site's four bytes.
containsSite :: StringSite -> Bool
containsSite site = case siteCensus site of
  Census (Extent label start end _) _ : _ -> label == siteImage site && siteAddress site >= start && siteAddress site + 4 <= end
  [] -> False

roleWord :: StringRole -> Word32
roleWord role = case role of
  DpsContinueJobElement -> 0x00626f4a
  CalcsubTaskName -> 0x00627573
  PlaysubTaskName -> 0x00627573
  UsbTaskName -> 0x00627375
  AssocResponseLogFormat -> 0x00626320
  ActionLogFormat -> 0x00626320
  HwMode80211bText -> 0x00623131

scannedImages :: [ScanImage]
scannedImages = [minBound .. maxBound]

extentBound :: [(ScanImage, Image)] -> Extent -> Bool
extentBound images (Extent label start end expected) = case imageFor label images of
  Just image -> extentMatches image (GuestAddress start) (GuestAddress end) expected
  Nothing -> False

extent :: ScanImage -> Word32 -> Word32 -> String -> Extent
extent label start end expected = Extent label start end (digest expected)

digest :: String -> SHA.Sha256
digest = either (error . ("retained string digest: " ++)) id . SHA.parseSha256Hex

stringSites :: [StringSite]
stringSites = [continueJob, calcsub, playsub, usb, assocResponse, action, hwMode80211b]

-- x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, app/connectivity/ptp/tu_00d96780.c,
-- ptp_process_dps_response (NONMATCHING). id5.dis d97a80/d97a84 form 0xd98500 in r0 for the call to
-- ptp_xml_seek_element [d96d4c,d96e84), which reads it only by ldrb [r5] and ldrb [r5, r4] until NUL;
-- its callee ptp_xml_seek_start_tag [d96780,d96a20) saves only r4/r11 and makes no call.
continueJob :: StringSite
continueJob =
  StringSite DpsContinueJobElement ScanImage5 0xd98508 ConsumedData
    [Census (extent ScanImage5 0xd98500 0xd9850c "93be81222e66bd8088167935d3243166ae149928d10f57584ee99375b001946e") [MovwMovt ScanImage5 0xd97a80]]
    [ extent ScanImage5 0xd97728 0xd97b24 "c631336a9140232e2209f4ae19112447e08ffbecbcc3c9ac32fa6fa480bbf8b0"
    , extent ScanImage5 0xd96d4c 0xd96e84 "13e339e157f02ed0c8b896edc62ef16ba037d616b209b187d4ed4d7e06bd5988"
    , extent ScanImage5 0xd96780 0xd96a20 "2f7883ceae3687b91ca191bdae74672aeb9201cd33ddfc08a02093268335c755"
    ]

-- Task names. Each creator stores the name in field +4 of a stack task descriptor and passes the
-- descriptor only to dispatch_create_task [725dd8,726098), which loads fields 0, 8, 0xc, 0x10, 0x14
-- and 0x18 but never +4. Its descriptor pointer reaches only dispatch_enter_nested_critical
-- [7235d8,7236b0) in r0 (725dfc), whose first call is through [[0x96bd20] + 0x1618].
-- dispatch_initialize_interfaces [725a04,725b54) stores loader A word 0x40010 (0x4004dae0) at
-- 0x96bd20; kernel_publish_interfaces [40430,409ac) stores kernel_core_index_adapter 0x412bc at
-- 0x4004dae0 + 0x1618, which calls 0x427b8, an mrc/compare that reads no argument.
taskCreation :: [Extent]
taskCreation =
  [ extent ScanImage4 0x725dd8 0x726098 "1432c11311a1a15437364f8d69372bec69a2e06357c94ca56e00c402fe96cf43"
  , extent ScanImage4 0x7235d8 0x7236b0 "b0e6aeb81c94b2452fc91ad3da77e36906b3fce7faadc7e904ac9c244fa09100"
  , extent ScanImage4 0x725a04 0x725b54 "881404e076973f648220140e49b2b7238dfa5e88c09a634b248c902c33daafaf"
  , extent ScanLoaderA 0x40010 0x40014 "9a9639ca31b3d750107e475b6fbd46cf7dfe70d00727d000949da7bbfe1c47e0"
  , extent ScanLoaderA 0x40430 0x409ac "28c0323505b8d766e3fb86386620e6a399ea767ef30b3d39b771a184c6d89806"
  , extent ScanLoaderA 0x412bc 0x412cc "588a8f7ceca8c41d2ca7986a688a72e3d05cfc3ebce9547046e1b14ac8486b48"
  , extent ScanLoaderA 0x427b8 0x427d4 "304e0eafee9b09b251f9f6c6fb9590a31ef115f472cb51d17778e29e94418740"
  ]

-- app/control/af/workflow/message_rate_modes.c, s54u_initialize_mode_message_task (exact),
-- [f49dc0,f49e30): f49df0/f49df4 store 0xf49fbc at r11 - 0x1c, field +4 of the descriptor at r11 - 0x20.
calcsub :: StringSite
calcsub =
  StringSite CalcsubTaskName ScanImage5 0xf49fc0 ConsumedData
    [Census (extent ScanImage5 0xf49fbc 0xf49fc4 "e8bea412b0024befbb1619d4ac5d4257962bf17ad2ba9f2b3792b544960938c3") [MovwMovt ScanImage5 0xf49df0]]
    (extent ScanImage5 0xf49dc0 0xf49e30 "dabaa58da78441ec88f32461ac6203b0301c15328a0732aded429729280af642" : taskCreation)

-- app/playback/engine/playsub_setup.c, playback_sub_create_task (exact), [125a1e8,125a260):
-- 125a220/125a224 store 0x125a7e4 at r11 - 0x24, field +4 of the descriptor at r11 - 0x28.
playsub :: StringSite
playsub =
  StringSite PlaysubTaskName ScanPlain 0x125a7e8 ConsumedData
    [Census (extent ScanPlain 0x125a7e4 0x125a7ec "068f6f109ce7827beaa1dd8e1d7835c61a1eff2aaa8bd2734157412bd200815c") [MovwMovt ScanPlain 0x125a220]]
    (extent ScanPlain 0x125a1e8 0x125a260 "1e5fa116513304c64bb57b90c89dd37e711a435ef7e42a40f4decd3f0af7e741" : taskCreation)

-- app/connectivity/ptp/tu_00d80960.c, FUN_00d80d44 (NONMATCHING), [d80d44,d80dd4): d80d80/d80d84
-- store 0xd80dd4 at r11 - 0x24, field +4 of the descriptor at r11 - 0x28.
usb :: StringSite
usb =
  StringSite UsbTaskName ScanImage5 0xd80dd4 ConsumedData
    [Census (extent ScanImage5 0xd80dd4 0xd80dd8 "aa58088040b7de83054e3dcc50b4770fa4bd3af58dafc44835d812acab910edc") [MovwMovt ScanImage5 0xd80d80]]
    (extent ScanImage5 0xd80d44 0xd80dd4 "30adbad634c687c33f9585a81a0da6e7cc972e97e6ee639562711313aa12b543" : taskCreation)

-- Formats without '%' passed in r1 to wpa_printf [13257ec,1325a00) (wpa_ring_log_printf.c, NONMATCHING),
-- which passes them only to ui_vsnprintf [bf8cdc,bf8d20) and ui_vsnprintf_reentrant [bf8c58,bf8cdc)
-- (libc_support_recovery.c, exact) as the format of FUN_00bf6e60 [bf6e60,bf8bf8). There the format
-- register r9 and its copies are read only by ldrb with post-increment until '%' or NUL, and the
-- literal run is queued at bf6f40 as an iov {base, length} that runtime_string_write_vectors
-- [bf5ed4,bf605c) hands to runtime_move_bytes [89d908,89da3c) (runtime_string_copy.c, exact).
formatReaders :: [Extent]
formatReaders =
  [ extent ScanPlain 0x13257ec 0x1325a00 "7149bae2fc35b77e5e3a26a2a53524ea208c2f4d59320a8e05a7bad65574f401"
  , extent ScanImage5 0xbf8cdc 0xbf8d20 "f63c55f23b3830ce8256d01ad127638d6f784b17caf8d305b82a6cf37f117788"
  , extent ScanImage5 0xbf8c58 0xbf8cdc "50d24060dc145c403137406be78f9f25d639b1285efd4e985bcbfcd42d9f0020"
  , extent ScanImage5 0xbf6e60 0xbf8bf8 "a5a2ca47745e5a62f8572956344daf65fae170d01d4a3132cb4d81603a015ff7"
  , extent ScanImage5 0xbf5ed4 0xbf605c "dde5698a0ee6d5fe0055344aee4b9611e357abda47c64193fd0236095aa5bb51"
  , extent ScanImage4 0x89d908 0x89da3c "e954ea5ddd602b99259e9d5b93ded286c5845bb546a45940b873a2209182dc8d"
  ]

-- app/third_party/hostap/src/ap/ieee802_11.c, ieee802_11_mgmt_cb (exact), [13a6018,13a6160).
mgmtCallback :: Extent
mgmtCallback = extent ScanPlain 0x13a6018 0x13a6160 "5da713112485cb4128e8aa2acf7e415a3372589f3359e43b410ccc5dc5499adb"

assocResponse :: StringSite
assocResponse =
  StringSite AssocResponseLogFormat ScanPlain 0x13a27d4 ConsumedData
    [Census (extent ScanPlain 0x13a27c4 0x13a27d8 "5dedfe6f0c3700a706107052c7b086605dd0c8685bdb4108fef2d35cb27611d4") [MovwMovt ScanPlain 0x13a60b0]]
    (mgmtCallback : formatReaders)

action :: StringSite
action =
  StringSite ActionLogFormat ScanPlain 0x13a2810 ConsumedData
    [Census (extent ScanPlain 0x13a2804 0x13a2814 "296739929c8e5f177fcf45dbbea314d3ae9407f66acc134a14d814ad9ce1f10c") [MovwMovt ScanPlain 0x13a6124]]
    (mgmtCallback : formatReaders)

-- [138d12c,138d148) is padding then "802.11a", "802.11g", "802.11b"; no handle names it. The
-- neighbours are formats without '%' read forward to their NUL by the format readers:
-- [138d104,138d12c) from ieee802_1x_tx_key [138e858,138eaa0) (ieee802_1x.c, exact) at 138ea7c, and
-- [138d148,138d176) from FUN_0138eaa0 [138eaa0,138ec30) at 138eaf0.
hwMode80211b :: StringSite
hwMode80211b =
  StringSite HwMode80211bText ScanPlain 0x138d144 UnreferencedData
    [ Census (extent ScanPlain 0x138d12c 0x138d148 "f45f6a6bea9082691263c0ed1caf011161ffb30e0f1d5f74c998cb87b297cad4") []
    , Census (extent ScanPlain 0x138d104 0x138d12c "871fa04273915cc0ffca8032f9973924bafadd8fbe330117a22b637642f74197") [MovwMovt ScanPlain 0x138ea7c]
    , Census (extent ScanPlain 0x138d148 0x138d176 "e2312e3653b2bf464dae3f8f5678a478a1ec8756c3081078178d7b6b90838a65") [MovwMovt ScanPlain 0x138eaf0]
    ]
    ( extent ScanPlain 0x138e858 0x138eaa0 "f38838e7e2e83ebb6570d951a714845f7b0a7a00764bacbec1b32a9388559d18"
        : extent ScanPlain 0x138eaa0 0x138ec30 "6090642e34898c259d978a86130f16953980e3bca27e4951614f445cda8f3c25"
        : formatReaders
    )
