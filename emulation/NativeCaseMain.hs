module Main where
import FujiAF.Host
import FujiAF.JSON (encodeCanonical)
import Emulation.NativeCase
import qualified Data.ByteString as B
import System.Directory
import System.Environment
import System.Exit
import System.FilePath
main :: IO ()
main=do
 args<-getArgs
 workspace<-lookupEnv "BUILD_WORKSPACE_DIRECTORY"
 let resolve input=if isRelative input then maybe input (</>input)workspace else input
 case args of
  ["--runner",runner,path]->do
   (code,report)<-runCase(resolve runner)(resolve path)
   jsonPrint report
   if code==0 then pure() else exitFailure
  ["--runner",runner,"--batch",batch,"--output",output]->do
   let directory=resolve output
   createDirectoryIfMissing True directory
   summary<-runBatch(resolve runner)(resolve batch)$ \report->
    B.writeFile(directory</>caseIdText(reportCase report)<.>"report.json")(encodeCanonical(reportValue report))
   B.writeFile(directory</>"batch-summary.json")(encodeCanonical(summaryJson summary))
   jsonPrint(summaryJson summary)
   if batchPassed summary then pure() else exitFailure
  _->jsonPrint(obj[("schema_version",n 1),("profile",s "unknown"),("status",s "invalid_case"),("reason",s "usage: native-case --runner RUNNER CASE | native-case --runner RUNNER --batch BATCH --output DIRECTORY")])>>exitFailure
