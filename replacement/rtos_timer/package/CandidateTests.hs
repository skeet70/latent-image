module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString as B
import Replacement.RtosTimer.Candidate
import qualified Replacement.StartupArtifact.Types as S

main :: IO ()
main = do
  let source = B.pack [1,2,3,4,5,6,7,8]
      property = B.pack [1,9,10,4,5,6,7,8]
      record = B.pack [1,2,3,4,5,11,7,8]
      expected = B.pack [1,9,10,4,5,11,7,8]
      collision = B.pack [1,12,3,4,5,6,7,8]
  propertyChanges <- admitted (changedExtents PropertyService source property)
  recordChanges <- admitted (changedExtents RecordStore source record)
  collisionChanges <- admitted (changedExtents RecordStore source collision)
  merged <- admitted (mergeImage4 [] source (propertyChanges ++ recordChanges))
  check "both owned services install without changing other bytes" (merged == expected)
  check "order does not change emitted image" (mergeImage4 [] source (recordChanges ++ propertyChanges) == Right expected)
  check "carrier collision refuses" (refuses (mergeImage4 [] source (propertyChanges ++ collisionChanges)))
  check "stale source resource refuses" (refuses (mergeImage4 [] (B.pack [1,0,3,4,5,6,7,8]) propertyChanges))
  check "wrong image extent refuses" (refuses (changedExtents PropertyService source (B.take 7 property)))
  check "unchanged allocated carrier bytes still collide"
    (refuses (rejectCarrierOverlap [S.Span S.Image4 (S.Guest 0x61c400) (S.Guest 0x61c404)]
      [S.Span S.Image4 (S.Guest 0x61c403) (S.Guest 0x61c408)]))
  check "startup carrier collision refuses" (refuses (mergeImage4 [S.Span S.Image4 (S.Guest 0x61c401) (S.Guest 0x61c403)] source propertyChanges))
  check "adjacent startup carrier is preserved" (mergeImage4 [S.Span S.Image4 (S.Guest 0x61c403) (S.Guest 0x61c405)] source propertyChanges == Right property)
  check "already installed artifact cannot impersonate original preimage" (refuses (mergeImage4 [] property propertyChanges))
  check "readback corruption refuses" (refuses (mergeImage4 [] (B.pack [1,9,0,4,5,6,7,8]) propertyChanges))
  check "inverse admits the emitted candidate" (admitImage4Inverse source expected (propertyChanges ++ recordChanges) == Right ())
  check "inverse rejects a corrupted installed byte"
    (refuses (admitImage4Inverse source (B.pack [1,9,0,4,5,11,7,8]) (propertyChanges ++ recordChanges)))
  check "inverse rejects a corrupted preserved resource"
    (refuses (admitImage4Inverse source (B.pack [1,9,10,4,5,11,7,0]) (propertyChanges ++ recordChanges)))
  putStrLn "candidate composition controls passed"
  where
    admitted = either (ioError . userError) pure
    check name result = unless result (ioError (userError name))
    refuses (Left _) = True
    refuses (Right _) = False
