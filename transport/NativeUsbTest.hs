{-# LANGUAGE ForeignFunctionInterface #-}
-- The native inventory through the real libusb. fuji_usb_list keeps the
-- device list in static state, so the second call in a process is where the
-- one-call-per-process assumption showed; the synthetic tests substitute the
-- UsbApi record and never run the C, so none of them can fail on it. This
-- test enumerates the host bus and nothing else: no device is opened, claimed
-- or transferred, and no camera is required. It also calls the shim's size
-- gate directly, which needs no library.
module Main where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (forM_, unless)
import Foreign.C.Types (CInt (..))
import FujiAF.Native (nativeApi)
import FujiAF.Transport (Candidate, UsbApi (..))

foreign import ccall unsafe "fuji_usb_transfer_size_admitted" sizeAdmitted :: CInt -> CInt -> IO CInt

check :: String -> Bool -> IO ()
check label yes = unless yes (error label)

stage :: String -> IO () -> IO ()
stage label action = either (\e -> error (label++" failed: "++displayException e)) pure =<< (try action :: IO (Either SomeException ()))

-- A listing that returns has a usable inventory: the provider reads the path
-- and every descriptor field of every entry through the C accessors, so a
-- refusal, a short read or an unreadable field cannot survive it.
listOnce :: UsbApi -> IO (Either SomeException [Candidate])
listOnce api = try (usbList api 0)

inventoryOf :: String -> Either SomeException [Candidate] -> IO [Candidate]
inventoryOf label result = either (\e -> error (label++" native listing failed: "++displayException e)) pure result

main :: IO ()
main = do
  -- Kinds: 0 control, 1 bulk OUT, 2 bulk IN.
  forM_ [ (2,1,True), (2,0x100000,True), (2,0x100001,False), (2,0,False)
        , (1,292,True), (1,293,False), (1,0x100000,False), (0,12,True), (0,293,False), (0,0x100000,False) ] $ \(kind,size,admitted) -> do
    result <- sizeAdmitted kind size
    check ("shim size gate for kind "++show kind++", "++show size++" bytes") ((result /= 0) == admitted)
  let api = nativeApi
  stage "native load and init" (usbInit api 0)
  first <- listOnce api
  second <- listOnce api
  third <- listOnce api
  stage "native exit" (usbExit api 0)
  firstInventory <- inventoryOf "first" first
  secondInventory <- inventoryOf "second" second
  thirdInventory <- inventoryOf "third" third
  check ("each listing returns the same inventory: "++show (map length [firstInventory,secondInventory,thirdInventory]))
    (firstInventory == secondInventory && secondInventory == thirdInventory)
  putStrLn ("native-usb: "++show (length firstInventory)++" host devices, listed three times with equal inventory")
