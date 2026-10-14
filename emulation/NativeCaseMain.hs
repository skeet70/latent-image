module Main where
import FujiAF.Host
import Emulation.NativeCase
import System.Environment
import System.Exit
import System.FilePath
main :: IO ()
main=do
 args<-getArgs
 case args of
  ["--runner",runner,path]->do
   workspace<-lookupEnv "BUILD_WORKSPACE_DIRECTORY"
   let resolve input=if isRelative input then maybe input (</>input)workspace else input
   (code,report)<-runCase(resolve runner)(resolve path)
   jsonPrint report
   if code==0 then pure() else exitFailure
  _->jsonPrint(obj[("schema_version",n 1),("profile",s "unknown"),("status",s "invalid_case"),("reason",s "usage: native-case --runner RUNNER CASE")])>>exitFailure
