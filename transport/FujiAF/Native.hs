{-# LANGUAGE ForeignFunctionInterface #-}
module FujiAF.Native (nativeApi, nativeApiWith, NativeCalls(..)) where

import Control.Exception (throwIO)
import Control.Monad (forM, unless, when)
import Foreign
import Foreign.C
import FujiAF.Transport
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

-- The native_usb.c entry points the provider marshals through.
data NativeCalls = NativeCalls
  { callLoad :: IO CInt, callInit :: IO CInt, callList :: IO CInt
  , callPath :: CInt -> IO CString, callField :: CInt -> CInt -> IO CInt
  , callOpen :: CString -> IO CInt, callClaim :: IO CInt
  , callTransfer :: CInt -> CInt -> Ptr Word8 -> CInt -> CUInt -> Ptr CInt -> IO CInt
  , callRelease :: IO CInt, callClose :: IO CInt, callExit :: IO CInt
  }

status :: String -> IO CInt -> IO ()
status label action = do
  result <- action
  unless (result == 0) (P.reject (label++" native status "++show result))

-- Constructing this record is inert. Only usbInit loads/initializes libusb.
nativeApi :: UsbApi
nativeApi = nativeApiWith (NativeCalls loadUSB initUSB listUSB pathUSB fieldUSB openUSB claimUSB transferUSB releaseUSB closeUSB exitUSB)

nativeApiWith :: NativeCalls -> UsbApi
nativeApiWith calls = UsbApi
  { usbInit = \_ -> do
      status "load" (callLoad calls)
      status "init" (callInit calls)
  , usbList = \_ -> do
      count <- callList calls
      when (count < 0 || count > 64) (P.reject ("native inventory status "++show count))
      forM [0..count-1] $ \i -> do
        path <- callPath calls i >>= peekCString
        values <- forM ([0..11]++[37,38]) (fmap fromIntegral . callField calls i)
        extraCount <- fromIntegral <$> callField calls i 12
        when (extraCount < 0 || extraCount > (6::Int)) (P.reject "extra endpoint cap")
        extras <- forM [0..extraCount-1] $ \j -> do
          fields <- forM [13+j*4..16+j*4] (fmap fromIntegral . callField calls i . fromIntegral)
          case fields of [a,b,c,d] -> pure (a,b,c,d); _ -> P.reject "native endpoint shape"
        case values of
          [vendor,productId,config,iface,alt,cls,sub,proto,ein,eout,pin,pout,iin,iout] -> pure (Candidate path vendor productId (Descriptors config iface alt cls sub proto ein eout pin pout iin iout extras))
          _ -> P.reject "native descriptor shape"
  , usbOpen = \device _ -> status "open" (withCString (physicalPath device) (callOpen calls))
  , usbClaim = \iface _ -> unless (iface==0) (P.reject "interface not allowed") >> status "claim" (callClaim calls)
  , usbTransfer = \packet milliseconds -> do
      (kind,endpoint,input,size) <- case packet of
        P.ControlWrite 0x43 0 0 0 bytes -> pure (0::CInt,0,Just bytes,length bytes)
        P.BulkOut ep bytes -> pure (1,ep,Just bytes,length bytes)
        P.BulkIn ep n -> pure (2,ep,Nothing,n)
        _ -> P.reject "unsupported native transfer"
      let withBuffer = maybe (allocaArray size) withArray input
      withBuffer $ \buffer -> alloca $ \countPtr -> do
        poke countPtr 0
        result <- callTransfer calls kind (fromIntegral endpoint) buffer (fromIntegral size) (fromIntegral milliseconds) countPtr
        count <- fromIntegral <$> peek countPtr
        let validCount = count >= 0 && count <= size
        bytes <- if kind==2 && validCount then peekArray count buffer else pure []
        unless (validCount && result == 0) (throwIO (ProviderFailure (fromIntegral result) count bytes))
        pure (count,bytes)
  , usbRelease = \iface _ -> unless (iface==0) (P.reject "interface not allowed") >> status "release" (callRelease calls)
  , usbClose = \_ -> status "close" (callClose calls)
  , usbExit = \_ -> status "exit" (callExit calls)
  }
