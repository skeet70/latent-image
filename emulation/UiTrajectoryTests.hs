{-# LANGUAGE OverloadedStrings #-}
-- | Decoding of e2e trajectory files: the checked-in files and synthetic trace-site cases.
module Main (main) where

import Control.Monad (unless)
import qualified Data.ByteString.Char8 as C
import Data.List (isSuffixOf, sort)
import Emulation.UiTrajectory
import System.Directory (listDirectory)
import System.Exit (exitFailure)
import System.FilePath ((</>))

document :: String -> String -> C.ByteString
document sites requires = C.pack (concat
  [ "{\"schema\":1,\"trace_sites\":", sites
  , ",\"before_unload\":[{\"phase\":\"a\",\"steps\":[[\"menu\",1,5,2]]}"
  , ",{\"phase\":\"b\",\"requires\":", requires, ",\"steps\":[]}]"
  , ",\"after_unload\":[]}" ])

sitesOf :: C.ByteString -> Maybe [TraceSite]
sitesOf bytes = either (const Nothing) (Just . trajectoryTraceSites) (decodeTrajectories bytes)

refused :: C.ByteString -> Bool
refused = either (const True) (const False) . decodeTrajectories

expect :: String -> Bool -> IO Bool
expect name ok = do
  putStrLn ((if ok then "ok   " else "FAIL ") ++ name)
  pure ok

main :: IO ()
main = do
  files <- sort . filter ("-trajectories.json" `isSuffixOf`) <$> listDirectory "emulation"
  checkedIn <- mapM (\file -> do
      decoded <- decodeTrajectories <$> C.readFile ("emulation" </> file)
      expect ("checked-in " ++ file ++ " decodes") (either (const False) (const True) decoded))
    files
  results <- sequence
    [ expect "the checked-in trajectory files are present" (not (null files))
    , expect "a trace site decodes to its address and label" $
        sitesOf (document "[[\"0114b6c4\",\"film-icon\"]]" "[\"a\"]")
          == Just [TraceSite 0x0114b6c4 "film-icon"]
    , expect "a file without trace sites declares none" $
        sitesOf (C.pack "{\"schema\":1,\"before_unload\":[],\"after_unload\":[]}") == Just []
    , expect "an unaligned trace site is refused" $
        refused (document "[[\"0114b6c6\",\"film-icon\"]]" "[]")
    , expect "a trace site address with 0x is refused" $
        refused (document "[[\"0x114b6c4\",\"film-icon\"]]" "[]")
    , expect "a reserved trace label is refused" $
        refused (document "[[\"0114b6c4\",\"menu-message\"]]" "[]")
    , expect "a repeated trace label is refused" $
        refused (document "[[\"0114b6c4\",\"x\"],[\"01146cf0\",\"x\"]]" "[]")
    , expect "a repeated trace address is refused" $
        refused (document "[[\"0114b6c4\",\"x\"],[\"0114b6c4\",\"y\"]]" "[]")
    , expect "a requirement naming no segment is refused" $
        refused (document "[]" "[\"missing\"]")
    ]
  unless (and (checkedIn ++ results)) exitFailure
