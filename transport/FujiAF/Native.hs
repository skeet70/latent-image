{-# LANGUAGE ForeignFunctionInterface #-}
module FujiAF.Native (nativeApi, nativeApiJournaling, OutcomeJournal(..)) where

import Control.Monad (forM, unless, when)
import Foreign
import Foreign.C
import FujiAF.Transport
import FujiAF.Read (readReplyBytes)
import qualified FujiAF.Broker as P

foreign import ccall safe "fuji_usb_load" loadUSB :: IO CInt
foreign import ccall safe "fuji_usb_init" initUSB :: IO CInt
foreign import ccall safe "fuji_usb_list" listUSB :: IO CInt
foreign import ccall unsafe "fuji_usb_path" pathUSB :: CInt -> IO CString
foreign import ccall unsafe "fuji_usb_field" fieldUSB :: CInt -> CInt -> IO CInt
foreign import ccall safe "fuji_usb_open" openUSB :: CString -> IO CInt
foreign import ccall safe "fuji_usb_claim" claimUSB :: IO CInt
foreign import ccall safe "fuji_usb_transfer" transferUSB :: CInt -> CInt -> Ptr Word8 -> CInt -> CUInt -> Ptr CInt -> IO CInt
foreign import ccall safe "fuji_usb_release" releaseUSB :: IO CInt
foreign import ccall safe "fuji_usb_close" closeUSB :: IO CInt
foreign import ccall safe "fuji_usb_exit" exitUSB :: IO CInt

status :: String -> IO CInt -> IO ()
status label action = do
  result <- action
  unless (result == 0) (P.reject (label++" native status "++show result))

-- Transfer outcomes are journaled per transfer for the per-frame commands.
-- dump-ram pre-announces its frame sequence and journals one summary line per
-- request, so its provider journals only a failing or short outcome.
data OutcomeJournal = EveryOutcome | FailingOutcomes deriving (Eq, Show)

-- Constructing this record is inert. Only usbInit loads/initializes libusb.
nativeApi :: Int -> FilePath -> UsbApi
nativeApi = nativeApiJournaling EveryOutcome

nativeApiJournaling :: OutcomeJournal -> Int -> FilePath -> UsbApi
nativeApiJournaling outcomeJournal journalCap dir = UsbApi
  { usbInit = \_ -> do
      status "load" loadUSB
      status "init" initUSB
  , usbList = \_ -> do
      count <- listUSB
      when (count < 0 || count > 64) (P.reject ("native inventory status "++show count))
      forM [0..count-1] $ \i -> do
        path <- pathUSB i >>= peekCString
        values <- forM ([0..11]++[37,38]) (fmap fromIntegral . fieldUSB i)
        extraCount <- fromIntegral <$> fieldUSB i 12
        when (extraCount < 0 || extraCount > (6::Int)) (P.reject "extra endpoint cap")
        extras <- forM [0..extraCount-1] $ \j -> do
          fields <- forM [13+j*4..16+j*4] (fmap fromIntegral . fieldUSB i . fromIntegral)
          case fields of [a,b,c,d] -> pure (a,b,c,d); _ -> P.reject "native endpoint shape"
        case values of
          [vendor,productId,config,iface,alt,cls,sub,proto,ein,eout,pin,pout,iin,iout] -> pure (Candidate path vendor productId (Descriptors config iface alt cls sub proto ein eout pin pout iin iout extras))
          _ -> P.reject "native descriptor shape"
  , usbOpen = \device _ -> status "open" (withCString (physicalPath device) openUSB)
  , usbClaim = \iface _ -> unless (iface==0) (P.reject "interface not allowed") >> status "claim" claimUSB
  , usbTransfer = \packet milliseconds -> do
      let (kind,endpoint,input,size) = case packet of
            P.ControlWrite 0x43 0 0 0 bytes -> (0::Int,0,bytes,length bytes)
            P.BulkOut ep bytes -> (1,ep,bytes,length bytes)
            P.BulkIn ep n -> (2,ep,replicate n 0,n)
            _ -> (3,0,[],0)
      unless (kind < 3 && size > 0 && size <= readReplyBytes) (P.reject "unsupported native transfer")
      withArray input $ \buffer -> alloca $ \countPtr -> do
        poke countPtr 0
        result <- transferUSB (fromIntegral kind) (fromIntegral endpoint) buffer (fromIntegral size) (fromIntegral milliseconds) countPtr
        count <- fromIntegral <$> peek countPtr
        let validCount = count >= 0 && count <= size
        bytes <- if kind==2 && validCount then peekArray count buffer else pure []
        -- Preserve negative API status and any partial bytes before rejecting.
        when (outcomeJournal == EveryOutcome || result /= 0 || count /= size || not validCount) $
          recordWithin journalCap dir "native-outcome" (show (result,count,P.hexBytes bytes))
        unless validCount (P.reject "invalid native byte count; raw count/status journaled without dereferencing bytes")
        unless (result==0) (P.reject ("native transfer status "++show result++"; partial count="++show count++"; no retry"))
        pure (count,bytes)
  , usbRelease = \iface _ -> unless (iface==0) (P.reject "interface not allowed") >> status "release" releaseUSB
  , usbClose = \_ -> status "close" closeUSB
  , usbExit = \_ -> status "exit" exitUSB
  }
