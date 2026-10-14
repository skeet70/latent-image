{-# LANGUAGE ScopedTypeVariables #-}
module Main where
import FujiAF.Host
import AF.Simulation
import Control.Exception
import System.Environment
import System.Exit
import System.FilePath
import System.IO

main :: IO ()
main = run `catch` \(e::IOException)->hPutStrLn stderr("synthetic AF: "++show e)>>exitFailure
run :: IO ()
run=do
 args<-getArgs
 case args of
  binary:checker:rest->do
   let paths=filter(/="--trace")rest
   ensure(length paths==1) "usage: simulate BINARY SCHEMA-CHECKER FIXTURE [--trace]"
   workspace<-lookupEnv "BUILD_WORKSPACE_DIRECTORY"
   input<-case paths of [p]->pure p; _->failH "one fixture required"
   let path=if isRelative input then maybe input (</>input) workspace else input
   fixture<-readRegular(1024*1024)path >>= jsonRead
   let schemaRoot=maybe "." id workspace
   schemaCheck checker (schemaRoot </> "schemas/af-simulation.schema.json") [fixture]
   (result,trace)<-simulate binary fixture
   schemaCheck checker (schemaRoot </> "schemas/af-observation-action.schema.json") trace
   jsonPrint(if "--trace" `elem` rest then set "trace" (arr trace) result else result)
  _->failH "usage: simulate BINARY SCHEMA-CHECKER FIXTURE [--trace]"
