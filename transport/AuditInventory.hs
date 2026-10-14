{-# LANGUAGE OverloadedStrings #-}
-- Static public source/runtime byte inventory. No loader or native API calls.
module Main where
import Control.Monad (forM)
import Data.Aeson (Value,object,(.=))
import qualified Data.Aeson.KeyMap as M
import Data.Aeson (Value(Object))
import qualified Data.ByteString as B
import System.Environment (getArgs)
import System.FilePath
import FujiAF.JSON (encodeCanonical)
import FujiAF.SHA256 (sha256)
import qualified FujiAF.Broker as P

sources :: [(String,String,String,FilePath,[FilePath])]
sources =
 [("libusb","1.0.30","87a55632db62c9bdc58cd31d3ccfa673f1bb017f","/tmp/libusb-1.0.30",["libusb/libusb.h","libusb/libusbi.h","libusb/core.c","libusb/descriptor.c","libusb/sync.c","libusb/io.c","libusb/hotplug.c","libusb/os/linux_usbfs.c","libusb/os/linux_usbfs.h","libusb/os/linux_udev.c","libusb/os/events_posix.c","libusb/os/threads_posix.c"])
 ,("systemd-libudev","261.2","4925d9f07fc697efccd98a93046ff535b8832445","/tmp/systemd-261.2",["src/libudev/libudev.c","src/libudev/libudev-device.c","src/libudev/libudev-enumerate.c","src/libudev/libudev-list.c","src/libudev/libudev-monitor.c","src/libc/libc-shim.h","src/libsystemd/sd-device/sd-device.c","src/libsystemd/sd-device/device-private.c","src/libsystemd/sd-device/device-enumerator.c","src/libsystemd/sd-device/device-monitor.c"])]

describe :: FilePath -> IO Value
describe path = do
 bytes<-P.readBounded (64*1024*1024) path
 pure (object ["size_bytes" .= B.length bytes,"sha256" .= sha256 bytes])
merge :: Value -> Value -> Value
merge (Object a) (Object b)=Object (M.union a b)
merge _ _=error "inventory object"
main :: IO ()
main = do
 args<-getArgs
 output<-case args of [file]->pure file; _->P.reject "usage: audit-inventory OUTPUT_JSON (fixed public source and ELF byte paths)"
 sourceValues<-forM sources $ \(name,version,commit,checkout,paths)->do
  files<-forM paths $ \path->merge (object ["path" .= path]) <$> describe (checkout </> path)
  pure (object ["name" .= name,"version" .= version,"commit" .= commit,"license" .= ("LGPL-2.1-or-later"::String),"files" .= files])
 archives<-forM [("libusb-1.0.30","/tmp/fuji-af-libusb-1.0.30.tar.gz","https://github.com/libusb/libusb/archive/refs/tags/v1.0.30.tar.gz"),("systemd-261.2","/tmp/fuji-af-systemd-261.2.tar.gz","https://github.com/systemd/systemd/archive/refs/tags/v261.2.tar.gz")] $ \(name,path,url)->merge (object ["name" .= (name::String),"url" .= (url::String)]) <$> describe path
 runtimes<-forM [("libusb","/nix/store/izifjzcz7vfmr1yfdrzmjqyxvpjrpy4m-libusb-1.0.30/lib/libusb-1.0.so.0.6.0"),("libudev","/nix/store/g909pi91zmvb7zp27cvn6ip27s2mr48q-systemd-minimal-libs-261.2/lib/libudev.so.1.7.14"),("libc","/nix/store/lm3pknxi0ipypy3lxh1wmm8wvvavdwrn-glibc-2.42-84/lib/libc.so.6"),("libpthread","/nix/store/lm3pknxi0ipypy3lxh1wmm8wvvavdwrn-glibc-2.42-84/lib/libpthread.so.0"),("loader","/nix/store/lm3pknxi0ipypy3lxh1wmm8wvvavdwrn-glibc-2.42-84/lib/ld-linux-x86-64.so.2")] $ \(name,path)->merge (object ["name" .= (name::String),"path" .= (path::String),"scope" .= ("ELF bytes and linked-dependency identity; not whole-library/OS certification"::String)]) <$> describe path
 let value=object ["schema_version" .= (1::Int),"scope" .= ("source-inspected invoked host transport paths; static runtime identity; no native load or camera observation"::String),"sources" .= sourceValues,"archives" .= archives,"runtimes" .= runtimes]
 B.writeFile output (encodeCanonical value <> "\n")
