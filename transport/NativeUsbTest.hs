-- The native inventory through the real libusb. fuji_usb_list keeps the
-- device list in static state, so the second call in a process is where the
-- one-call-per-process assumption showed; the synthetic tests substitute the
-- UsbApi record and never run the C, so none of them can fail on it. This
-- test enumerates the host bus and nothing else: no device is opened, claimed
-- or transferred, and no camera is required.
module Main where

import Control.Exception (SomeException, displayException, try)
import Control.Monad (unless)
import FujiAF.Native (nativeApi)
import FujiAF.Transport (Candidate, UsbApi (..))

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
